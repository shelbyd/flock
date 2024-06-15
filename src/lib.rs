use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use eyre::{Context, OptionExt};

pub type Word = u64;
pub type Stack = Vec<Word>;

pub async fn execute_at_path(path: &Path) -> eyre::Result<Word> {
    // TODO(shelbyd): Catch panics?
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Reading {}", path.display()))?;

    let program = parse(&contents)?;

    Ok(execute(&program).await?)
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

    let parse = |s: &str| ValSp::parse(s, &labels);

    let mut ops = Vec::new();
    for line in relevant_lines {
        if line.starts_with(":") {
            continue;
        }

        let (command, args) = match line.split_once(" ") {
            None => (line, Vec::new()),
            Some((command, args)) => (command, args.split(", ").collect()),
        };

        let op = match (command, &args[..]) {
            ("PUSH", [v]) => OpCode::Push(parse(v)?),
            ("STORE", [addr, v]) => OpCode::Store(parse(addr)?, parse(v)?),
            ("LOAD", [addr]) => OpCode::Load(parse(addr)?),

            ("ADD", [a, b]) => OpCode::Add(parse(a)?, parse(b)?),
            ("SUB", [a, b]) => OpCode::Sub(parse(a)?, parse(b)?),
            ("MUL", [a, b]) => OpCode::Mul(parse(a)?, parse(b)?),

            ("JUMP", [addr]) => OpCode::Jump(parse(addr)?),
            ("JUMP_EQ", [a, b, addr]) => OpCode::JumpEq(parse(a)?, parse(b)?, parse(addr)?),

            ("FORK", [addr]) => OpCode::Fork(parse(addr)?),
            ("JOIN", [id]) => OpCode::Join(parse(id)?),
            ("THREAD_FINISH", [v]) => OpCode::ThreadFinish(parse(v)?),

            ("EXIT", [v]) => OpCode::Exit(parse(v)?),
            ("ASSERT_EQ", [a, b]) => OpCode::AssertEq(parse(a)?, parse(b)?),

            ("DEBUG", []) => OpCode::Debug,

            _ => eyre::bail!("Could not parse command: {line:?}"),
        };
        ops.push(op);
    }

    Ok(Program { ops })
}

#[derive(Debug)]
struct Program {
    ops: Vec<OpCode>,
}

#[derive(Debug)]
enum OpCode {
    Push(ValSp),
    Store(ValSp, ValSp),
    Load(ValSp),

    Add(ValSp, ValSp),
    Sub(ValSp, ValSp),
    Mul(ValSp, ValSp),

    Jump(ValSp),
    JumpEq(ValSp, ValSp, ValSp),

    Fork(ValSp),
    Join(ValSp),
    ThreadFinish(ValSp),

    Exit(ValSp),
    AssertEq(ValSp, ValSp),

    Debug,
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

async fn execute(program: &Program) -> eyre::Result<Word> {
    let mut state = ThreadState::new();

    while let Some(op) = program.ops.get(state.instruction_pointer as usize) {
        state.instruction_pointer += 1;

        match op {
            OpCode::Push(v) => {
                let v = state.get(v)?;
                state.stack.push(v)
            }
            OpCode::Store(addr, v) => {
                let addr = state.get(addr)?;
                let v = state.get(v)?;
                state.write_memory(addr, v)?;
            }
            OpCode::Load(addr) => {
                let addr = state.get(addr)?;
                let v = state.read_memory(addr)?;
                state.stack.push(v);
            }

            OpCode::Add(a, b) => {
                let a = state.get(a)?;
                let b = state.get(b)?;
                state.stack.push(a + b);
            }
            OpCode::Sub(a, b) => {
                let a = state.get(a)?;
                let b = state.get(b)?;
                state.stack.push(a - b);
            }
            OpCode::Mul(a, b) => {
                let a = state.get(a)?;
                let b = state.get(b)?;
                state.stack.push(a * b);
            }

            OpCode::Jump(addr) => {
                let addr = state.get(addr)?;
                state.jump_to(addr, &program)?;
            }
            OpCode::JumpEq(a, b, addr) => {
                let a = state.get(a)?;
                let b = state.get(b)?;
                let addr = state.get(addr)?;

                if a == b {
                    state.jump_to(addr, &program)?;
                }
            }

            OpCode::Fork(addr) => {
                let addr = state.get(addr)?;
                todo!();
            }

            OpCode::Exit(code) => return Ok(state.get(code)?),
            OpCode::AssertEq(a, b) => {
                let a = state.get(a)?;
                let b = state.get(b)?;
                eyre::ensure!(a == b, "Expected {a} to equal {b}");
            }

            OpCode::Debug => {
                eprintln!("Stack");
                for (i, w) in state.stack.iter().rev().enumerate() {
                    eprintln!("{i}: {w}");
                }
                eprintln!("");
            }

            #[cfg(debug_assertions)]
            #[allow(unreachable_patterns)]
            _ => todo!("{op:?}"),
        }
    }

    Ok(0)
}

#[derive(Debug)]
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

    fn get(&mut self, val_sp: &ValSp) -> eyre::Result<Word> {
        match val_sp {
            ValSp::Literal(v) => Ok(*v),

            ValSp::Pop => self.stack.pop().ok_or_eyre("Pop from empty stack"),
            ValSp::Peek => self.stack.last().cloned().ok_or_eyre("Peek empty stack"),

            ValSp::Memory(addr) => {
                let addr = self.get(addr)?;
                Ok(self.read_memory(addr)?)
            }
        }
    }

    fn read_memory(&self, addr: Word) -> eyre::Result<Word> {
        let addr = self.aligned_local(addr)?;
        Ok(*self.memory.get(&addr).unwrap_or(&0))
    }

    fn aligned_local(&self, addr: Word) -> eyre::Result<Word> {
        const WORD_SIZE: Word = core::mem::size_of::<Word>() as Word;

        eyre::ensure!(addr % WORD_SIZE == 0, "Misaligned address: 0x{addr:x}");
        eyre::ensure!(
            addr >> (WORD_SIZE * 8 - 1) == 0,
            "Global addresses not supported yet: 0x{addr:x}"
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
