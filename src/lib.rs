use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
};

use eyre::{Context as _, OptionExt};
use tokio::{sync::RwLock, task::JoinHandle};

pub type Word = u64;
pub type Stack = Vec<Word>;

type Memory = BTreeMap<Word, Word>;

pub async fn execute_at_path(path: &Path) -> eyre::Result<Word> {
    // TODO(shelbyd): Catch panics?
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Reading {}", path.display()))?;

    let program = Arc::new(parse(&contents)?);

    match execute(program, ThreadState::new(), Default::default()).await? {
        ThreadResult::Exit(code) => Ok(code),
        ThreadResult::Finish(value) => Ok(value),
    }
}

fn parse(s: &str) -> eyre::Result<Program> {
    let relevant_lines = s
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter(|s| !s.starts_with("#"))
        .collect::<Vec<_>>();

    let mut ops_seen = 0;
    let mut labels = HashMap::new();
    for line in &relevant_lines {
        if let Some(label) = line.strip_prefix(":") {
            let existing = labels.insert(label, ops_seen);
            eyre::ensure!(existing.is_none(), "Duplicate label: {label}");
        } else {
            ops_seen += 1;
        }
    }

    let mut ops = Vec::new();
    for line in relevant_lines {
        if line.starts_with(":") {
            continue;
        }

        let (command, args) = match line.split_once(" ") {
            None => (line, Vec::new()),
            Some((command, args)) => (command, args.split(", ").collect()),
        };

        let op = OpCode::parse(command, &args, &labels)?;
        ops.push(op);
    }

    Ok(Program { ops })
}

#[derive(Debug)]
struct Program {
    ops: Vec<OpCode>,
}

#[derive(Debug)]
enum ValSp {
    // TODO(shelbyd): Indexed pop, $pop[3]
    Pop,
    Peek,

    Memory(Box<ValSp>),

    Literal(Word),
}

impl ValSp {
    fn parse(s: &str, labels: &HashMap<&str, u64>) -> eyre::Result<ValSp> {
        if let Some(label) = s.strip_prefix(":") {
            return Ok(ValSp::Literal(
                *labels
                    .get(label)
                    .ok_or_eyre(format!("Unknown label: {label}"))?,
            ));
        }

        if s == "$pop" {
            return Ok(ValSp::Pop);
        }

        if s == "$peek" {
            return Ok(ValSp::Peek);
        }

        if let Some(addr) = s.strip_prefix("$mem[").and_then(|s| s.strip_suffix("]")) {
            return Ok(ValSp::Memory(Box::new(ValSp::parse(addr, labels)?)));
        }

        Ok(ValSp::Literal(parse_literal(s)?))
    }
}

fn parse_literal(s: &str) -> eyre::Result<Word> {
    if let Some(hex) = s.strip_prefix("0x") {
        let v = Word::from_str_radix(hex, 16).context(format!("Parsing as hex: {hex:?}"))?;
        return Ok(v);
    }

    if let Ok(v) = s.parse() {
        return Ok(v);
    }

    eyre::bail!("Could not parse as literal value: {s:?}")
}

async fn execute(
    program: Arc<Program>,
    mut state: ThreadState,
    global_memory: Arc<RwLock<Memory>>,
) -> eyre::Result<ThreadResult> {
    let mut ctx = Context {
        state: &mut state,
        program: &program,
        global_memory,
        child_threads: HashMap::new(),
        next_child_id: 1,
    };

    while let Some(op) = program.ops.get(ctx.state.instruction_pointer as usize) {
        ctx.state.instruction_pointer += 1;

        if let Some(r) = op.execute(&mut ctx).await? {
            return Ok(r);
        }
    }

    Ok(ThreadResult::Exit(0))
}

// TODO(shelbyd): Move to interface for simulation.
fn spawn_execute(
    program: &Arc<Program>,
    state: ThreadState,
    global_memory: &Arc<RwLock<Memory>>,
) -> JoinHandle<eyre::Result<ThreadResult>> {
    let program = Arc::clone(program);
    let global_memory = Arc::clone(global_memory);
    tokio::task::spawn(async move { execute(program, state, global_memory).await })
}

#[derive(Debug, Clone)]
struct ThreadState {
    stack: Vec<Word>,
    memory: BTreeMap<Word, Word>,
    instruction_pointer: u64,
}

impl ThreadState {
    fn new() -> Self {
        ThreadState {
            stack: Default::default(),
            memory: Default::default(),
            instruction_pointer: 0,
        }
    }

    fn push(&mut self, v: Word) {
        self.stack.push(v);
    }

    fn read_memory(&self, addr: Word) -> eyre::Result<Word> {
        let addr = self.aligned_local(addr)?;
        Ok(*self.memory.get(&addr).unwrap_or(&0))
    }

    fn aligned_local(&self, addr: Word) -> eyre::Result<Word> {
        const WORD_SIZE: Word = core::mem::size_of::<Word>() as Word;

        eyre::ensure!(addr % WORD_SIZE == 0, "Misaligned address: 0x{addr:x}");
        eyre::ensure!(
            addr.leading_ones() == 0,
            "Attempted to access global address in state: 0x{addr:x}"
        );

        Ok(addr / WORD_SIZE)
    }

    fn write_memory(&mut self, addr: Word, value: Word) -> eyre::Result<()> {
        let addr = self.aligned_local(addr)?;
        self.memory.insert(addr, value);
        Ok(())
    }

    fn jump_to(&mut self, addr: Word, program: &Program) -> eyre::Result<()> {
        eyre::ensure!(
            (addr as usize) < program.ops.len(),
            "Jump outside of program range: {addr} >= {}",
            program.ops.len()
        );
        self.instruction_pointer = addr;

        Ok(())
    }
}

#[derive(Debug)]
enum ThreadResult {
    Exit(Word),
    Finish(Word),
}

struct Context<'a> {
    state: &'a mut ThreadState,
    program: &'a Arc<Program>,
    global_memory: Arc<RwLock<Memory>>,

    child_threads: HashMap<Word, JoinHandle<eyre::Result<ThreadResult>>>,
    next_child_id: Word,
}

impl Context<'_> {
    async fn get(&mut self, val_sp: &ValSp) -> eyre::Result<Word> {
        match val_sp {
            ValSp::Literal(v) => Ok(*v),

            ValSp::Pop => self.state.stack.pop().ok_or_eyre("Pop from empty stack"),
            ValSp::Peek => self
                .state
                .stack
                .last()
                .cloned()
                .ok_or_eyre("Peek empty stack"),

            ValSp::Memory(addr) => {
                let addr = Box::pin(self.get(addr)).await?;
                Ok(self.read_memory(addr).await?)
            }
        }
    }

    async fn read_memory(&self, addr: Word) -> eyre::Result<Word> {
        match self.aligned(addr)? {
            Address::Local(a) => Ok(self.state.read_memory(a)?),
            Address::Global(a) => Ok(*self.global_memory.read().await.get(&a).unwrap_or(&0)),
        }
    }

    fn aligned(&self, addr: Word) -> eyre::Result<Address> {
        const WORD_SIZE: Word = core::mem::size_of::<Word>() as Word;

        eyre::ensure!(addr % WORD_SIZE == 0, "Misaligned address: 0x{addr:x}");

        if addr >> (WORD_SIZE * 8 - 1) == 0 {
            Ok(Address::Local(addr))
        } else {
            Ok(Address::Global(addr))
        }
    }

    async fn write_memory(&mut self, addr: Word, val: Word) -> eyre::Result<()> {
        match self.aligned(addr)? {
            Address::Local(a) => Ok(self.state.write_memory(a, val)?),
            Address::Global(a) => {
                self.global_memory.write().await.insert(a, val);
                Ok(())
            }
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
enum Address {
    Local(Word),
    Global(Word),
}

macro_rules! op_codes {
    ({$($name: ident => |$ctx:ident, $($arg:ident),*| $body:tt)*}) => {
        #[allow(non_camel_case_types)]
        #[derive(Debug)]
        enum OpCode {
            $($name {
                $($arg: ValSp),*
            }),*
        }

        impl OpCode {
            fn parse(command: &str, args: &[&str], labels: &HashMap<&str, Word>) -> eyre::Result<OpCode> {
                match command {
                    $(stringify!($name) => {
                        let mut args_iter = args.iter();
                        let result = OpCode::$name {
                            $($arg: ValSp::parse(
                                args_iter.next().ok_or_eyre(format!("Too few arguments to {command}"))?,
                                labels)?
                            ),*
                        };
                        eyre::ensure!(args_iter.next().is_none(), "Too many arguments to {command}");
                        Ok(result)
                    })*

                    _ => eyre::bail!("Unknown command: {command}"),
                }
            }

            async fn execute(&self, ctx: &mut Context<'_>) -> eyre::Result<Option<ThreadResult>> {
                match self {
                    $(OpCode::$name { $($arg),* } => {
                        $(let $arg = ctx.get($arg).await?;)*
                        let $ctx = ctx;

                        $body;
                    })*
                }

                Ok(None)
            }
        }
    };
}

op_codes!({
    PUSH => |ctx, v| {
        ctx.state.push(v);
    }
    STORE => |ctx, addr, v| {
        ctx.write_memory(addr, v).await?;
    }
    LOAD => |ctx, addr| {
        let v = ctx.read_memory(addr).await?;
        ctx.state.push(v);
    }

    ADD => |ctx, a, b| {
        ctx.state.push(a + b);
    }
    SUB => |ctx, a, b| {
        ctx.state.push(a - b);
    }
    MUL => |ctx, a, b| {
        ctx.state.push(a * b);
    }

    JUMP => |ctx, addr| {
        ctx.state.jump_to(addr, ctx.program)?;
    }
    JUMP_EQ => |ctx, a, b, addr| {
        if a == b {
            ctx.state.jump_to(addr, ctx.program)?;
        }
    }

    FORK => |ctx, addr| {
        let mut fork_state = ctx.state.clone();
        fork_state.jump_to(addr, &ctx.program)?;
        fork_state.push(0);

        // TODO(shelbyd): Global task id.
        ctx.state.push(ctx.next_child_id);

        ctx.child_threads.insert(
            ctx.next_child_id,
            spawn_execute(&ctx.program, fork_state, &ctx.global_memory)
        );
        ctx.next_child_id += 1;
    }
    JOIN => |ctx, tid| {
        let handle = ctx.child_threads
            .remove(&tid)
            .ok_or_eyre(format!("Attempt to join unknown thread: {tid}"))?;

        match handle.await?? {
            // TODO(shelbyd): Exit from child thread without join.
            ThreadResult::Exit(e) => return Ok(Some(ThreadResult::Exit(e))),
            ThreadResult::Finish(v) => ctx.state.push(v),
        }
    }
    THREAD_FINISH => |_ctx, v| {
        return Ok(Some(ThreadResult::Finish(v)));
    }

    EXIT => |_ctx, v| {
        return Ok(Some(ThreadResult::Exit(v)));
    }
    ASSERT_EQ => |_ctx, a, b| {
        eyre::ensure!(a == b, "Expected {a} to equal {b}");
    }

    DEBUG => |ctx, | {
        eprintln!("Stack");
        for (i, w) in ctx.state.stack.iter().rev().enumerate() {
            eprintln!("{i}: {w}");
        }
        eprintln!("");
    }
});
