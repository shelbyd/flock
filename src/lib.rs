use std::{collections::BTreeMap, path::Path};

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
    let ops = s
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter(|s| !s.starts_with("#"))
        .map(|line| {
            let (command, args) = match line.split_once(" ") {
                None => (line, Vec::new()),
                Some((command, args)) => (command, args.split(", ").collect()),
            };

            Ok(match (command, &args[..]) {
                ("PUSH", [v]) => OpCode::Push(v.parse()?),
                ("STORE", [addr, v]) => OpCode::Store(addr.parse()?, v.parse()?),
                ("LOAD", [addr]) => OpCode::Load(addr.parse()?),

                ("ADD", [a, b]) => OpCode::Add(a.parse()?, b.parse()?),

                ("EXIT", [v]) => OpCode::Exit(v.parse()?),
                ("ASSERT_EQ", [a, b]) => OpCode::AssertEq(a.parse()?, b.parse()?),

                ("DEBUG", []) => OpCode::Debug,

                _ => eyre::bail!("Could not parse command: {line:?}"),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

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

impl std::str::FromStr for ValSp {
    type Err = eyre::Report;

    fn from_str(s: &str) -> eyre::Result<ValSp> {
        if s == "$pop" {
            return Ok(ValSp::Pop);
        }

        if s == "$peek" {
            return Ok(ValSp::Peek);
        }

        if let Some(addr) = s.strip_prefix("$mem[").and_then(|s| s.strip_suffix("]")) {
            return Ok(ValSp::Memory(Box::new(addr.parse()?)));
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
    let mut state = State::new();

    for op in &program.ops {
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

            OpCode::Exit(code) => return Ok(state.get(code)?),
            OpCode::AssertEq(a, b) => {
                let a = state.get(a)?;
                let b = state.get(b)?;
                if a != b {
                    return Ok(1);
                }
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
struct State {
    stack: Vec<Word>,
    memory: BTreeMap<Word, Word>,
}

impl State {
    fn new() -> Self {
        State {
            stack: Default::default(),
            memory: Default::default(),
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
}
