mod spawner;

use std::{
    collections::{BTreeMap, HashMap},
    ops::Deref,
    path::Path,
    sync::Arc,
};

use eyre::{Context as _, OptionExt};
use spawner::Spawner;
use tokio::sync::{Mutex, RwLock};

pub type Word = u64;
const WORD_SIZE: Word = core::mem::size_of::<Word>() as Word;

pub type Stack = Vec<Word>;

type Memory = BTreeMap<Word, Word>;

// What goes in Eal?
//   - Network
//   - Disk
//   ? RwLock / Mutex
/// External Abstraction Layer.
#[async_trait::async_trait]
pub trait Eal: Send + Sync + 'static {}

struct RealEal;

impl Eal for RealEal {}

struct HostCtx {
    #[allow(unused)]
    eal: Box<dyn Eal>,
    spawner: Box<dyn Spawner>,
    eprint: Mutex<()>,
}

impl HostCtx {
    pub async fn execute(self: &Arc<Self>, program: Program) -> eyre::Result<Word> {
        let process_ctx = Arc::new(ProcessCtx {
            host: Arc::clone(self),
            program,
            global_memory: Default::default(),
        });

        let root_id = process_ctx.spawn(ThreadState::new()).await?;
        match process_ctx.join(root_id).await? {
            ThreadResult::Exit(code) => Ok(code),
            ThreadResult::Finish(value) => Ok(value),
        }
    }
}

struct ProcessCtx {
    host: Arc<HostCtx>,
    program: Program,
    global_memory: RwLock<Memory>,
}

impl ProcessCtx {
    async fn spawn(self: &Arc<Self>, state: ThreadState) -> eyre::Result<Word> {
        Ok(self.spawner.spawn(self, state).await?)
    }

    async fn join(&self, tid: Word) -> eyre::Result<ThreadResult> {
        Ok(self.spawner.join(tid).await?)
    }
}

impl Deref for ProcessCtx {
    type Target = HostCtx;

    fn deref(&self) -> &Self::Target {
        &self.host
    }
}

struct ThreadCtx {
    proc: Arc<ProcessCtx>,
    id: Word,
    state: ThreadState,
}

impl ThreadCtx {
    async fn execute(mut self) -> eyre::Result<ThreadResult> {
        // TODO(shelbyd): Do we have to clone?
        let proc = Arc::clone(&self.proc);
        let ops = &proc.program.ops;

        loop {
            let Some(op) = ops.get(self.state.instruction_pointer as usize) else {
                return Ok(ThreadResult::Exit(0));
            };

            self.state.instruction_pointer += 1;

            if let Some(r) = op.execute(&mut self).await? {
                return Ok(r);
            }
        }
    }

    async fn get(&mut self, val_sp: &ValSp) -> eyre::Result<Word> {
        match val_sp {
            ValSp::Literal(v) => Ok(*v),

            ValSp::Pop => self.state.stack.pop().ok_or_eyre("Pop from empty stack"),
            ValSp::PopI(i) => {
                let i = Box::pin(self.get(i)).await? as usize;
                let index = self.state.stack.len() - i - 1;
                Ok(self.state.stack.remove(index as usize))
            }

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
            ValSp::GlobalMemory(addr) => {
                let addr = Box::pin(self.get(addr)).await?;
                Ok(self.read_memory(to_global(addr)).await?)
            }

            ValSp::ThreadId => Ok(self.id),
        }
    }

    async fn read_memory(&self, addr: Word) -> eyre::Result<Word> {
        match self.aligned(addr)? {
            Address::Local(a) => Ok(self.state.read_memory(a)?),
            Address::Global(a) => {
                let memory = &self.global_memory.read().await;
                Ok(*memory.get(&a).unwrap_or(&0))
            }
        }
    }

    fn aligned(&self, addr: Word) -> eyre::Result<Address> {
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

fn to_global(addr: u64) -> u64 {
    addr | (1 << (WORD_SIZE * 8 - 1))
}

impl Deref for ThreadCtx {
    type Target = ProcessCtx;

    fn deref(&self) -> &Self::Target {
        &self.proc
    }
}

pub async fn execute_at_path(path: &Path) -> eyre::Result<Word> {
    // TODO(shelbyd): Catch panics?
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Reading {}", path.display()))?;

    let program = Program::parse(&contents)?;
    execute_program(program, RealEal).await
}

pub async fn execute_program<E: Eal>(program: Program, eal: E) -> eyre::Result<Word> {
    let host = Arc::new(HostCtx {
        eal: Box::new(eal),
        spawner: Box::new(spawner::LocalSpawner::new()),
        eprint: Default::default(),
    });

    Ok(host.execute(program).await?)
}

#[derive(Debug, Clone)]
pub struct Program {
    ops: Vec<OpCode>,
}

impl Program {
    pub fn parse(s: &str) -> eyre::Result<Program> {
        let relevant_lines = s
            .lines()
            .map(|s| match s.split_once("#") {
                Some((pre, _)) => pre,
                None => s,
            })
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
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
}

#[derive(Debug, Clone)]
enum ValSp {
    Literal(Word),

    Pop,
    PopI(Box<ValSp>),

    Peek,

    Memory(Box<ValSp>),
    GlobalMemory(Box<ValSp>),

    ThreadId,
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

        let box_parse = |s| {
            eyre::Ok(Box::new(
                ValSp::parse(s, labels).context(format!("Parsing {s}"))?,
            ))
        };

        match indexed_expr("$pop", s)? {
            None => {}
            Some(None) => return Ok(ValSp::Pop),
            Some(Some(i)) => return Ok(ValSp::PopI(box_parse(i)?)),
        }

        if s == "$peek" {
            return Ok(ValSp::Peek);
        }

        match indexed_expr("$mem", s)? {
            None => {}
            Some(None) => eyre::bail!("$mem requires index"),
            Some(Some(addr)) => return Ok(ValSp::Memory(box_parse(addr)?)),
        }

        match indexed_expr("$gmem", s)? {
            None => {}
            Some(None) => eyre::bail!("$gmem requires index"),
            Some(Some(addr)) => return Ok(ValSp::GlobalMemory(box_parse(addr)?)),
        }

        if s == "$tid" {
            return Ok(ValSp::ThreadId);
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

fn strip_square_braces(s: &str) -> eyre::Result<Option<&str>> {
    let Some(without_first) = s.strip_prefix("[") else {
        return Ok(None);
    };

    let result = without_first
        .strip_suffix("]")
        .ok_or_eyre(format!("Expected ']' at end of: {s}"))?;

    Ok(Some(result))
}

fn indexed_expr<'s>(expr: &str, s: &'s str) -> eyre::Result<Option<Option<&'s str>>> {
    let Some(with_expr) = s.strip_prefix(expr) else {
        return Ok(None);
    };

    match strip_square_braces(with_expr)? {
        None => return Ok(Some(None)),
        Some(in_braces) => Ok(Some(Some(in_braces))),
    }
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

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
enum Address {
    Local(Word),
    Global(Word),
}

macro_rules! op_codes {
    ({$($name: ident => |$ctx:ident, $($arg:ident),*| $body:tt)*}) => {
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone)]
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
                            $($arg: {
                                let arg = args_iter.next().ok_or_eyre(format!("Too few arguments to {command}"))?;
                                ValSp::parse(arg, labels).context(format!("Parsing {arg}"))?
                            }),*
                        };
                        eyre::ensure!(args_iter.next().is_none(), "Too many arguments to {command}");
                        Ok(result)
                    })*

                    _ => eyre::bail!("Unknown command: {command}"),
                }
            }

            async fn execute(&self, ctx: &mut ThreadCtx) -> eyre::Result<Option<ThreadResult>> {
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

// TODO(shelbyd): Proc macro on functions.
op_codes!({
    NOP => |_ctx, _v| {}

    PUSH => |ctx, v| {
        ctx.state.push(v);
    }
    STORE => |ctx, addr, v| {
        ctx.write_memory(addr, v).await?;
    }
    STORE_GLOBAL => |ctx, addr, v| {
        ctx.write_memory(to_global(addr), v).await?;
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
    DIV => |ctx, a, b| {
        ctx.state.push(a / b);
    }

    SHIFT_LEFT => |ctx, a, b| {
        ctx.state.push(a << b);
    }

    JUMP => |ctx, addr| {
        ctx.state.jump_to(addr, &ctx.proc.program)?;
    }
    JUMP_EQ => |ctx, a, b, addr| {
        if a == b {
            ctx.state.jump_to(addr, &ctx.proc.program)?;
        }
    }

    FORK => |ctx, addr| {
        let mut fork_state = ctx.state.clone();
        fork_state.push(ctx.id);
        fork_state.jump_to(addr, &ctx.program)?;

        let child_id = ctx.proc.spawn(fork_state).await?;

        // TODO(shelbyd): Global task id.
        ctx.state.push(child_id);

    }
    JOIN => |ctx, tid| {
        match ctx.join(tid).await? {
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
        let _eprint = ctx.eprint.lock().await;

        eprintln!("Stack");
        for (i, w) in ctx.state.stack.iter().rev().enumerate() {
            eprintln!("{i}: 0x{w:x} ({w})");
        }
        eprintln!("");
    }
});
