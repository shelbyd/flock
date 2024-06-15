use std::path::Path;

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

    Add(ValSp, ValSp),

    Exit(ValSp),
    AssertEq(ValSp, ValSp),

    Debug,
}

#[derive(Debug)]
enum ValSp {
    // TODO(shelbyd): $peek
    // TODO(shelbyd): Indexed pop, $pop[3]
    Pop,
    Literal(Word),
}

impl ValSp {
    fn get(&self, stack: &mut Stack) -> eyre::Result<Word> {
        match self {
            ValSp::Pop => stack.pop().ok_or_eyre("Pop from empty stack"),
            ValSp::Literal(v) => Ok(*v),
        }
    }
}

impl std::str::FromStr for ValSp {
    type Err = eyre::Report;

    fn from_str(s: &str) -> eyre::Result<ValSp> {
        Ok(match s {
            "$pop" => ValSp::Pop,
            s => {
                if let Ok(lit) = s.parse() {
                    ValSp::Literal(lit)
                } else {
                    eyre::bail!("Could not parse as value specifier: {s:?}")
                }
            }
        })
    }
}

async fn execute(program: &Program) -> eyre::Result<Word> {
    let mut stack: Stack = Vec::new();

    for op in &program.ops {
        match op {
            OpCode::Push(v) => {
                let v = v.get(&mut stack)?;
                stack.push(v)
            }
            OpCode::Add(a, b) => {
                let a = a.get(&mut stack)?;
                let b = b.get(&mut stack)?;
                stack.push(a + b);
            }

            OpCode::Exit(code) => return Ok(code.get(&mut stack)?),
            OpCode::AssertEq(a, b) => {
                let a = a.get(&mut stack)?;
                let b = b.get(&mut stack)?;
                if a != b {
                    return Ok(1);
                }
            }

            OpCode::Debug => {
                eprintln!("Stack");
                for (i, w) in stack.iter().rev().enumerate() {
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
