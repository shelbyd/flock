use std::path::Path;

use eyre::{Context, OptionExt};

pub type Word = u64;

pub async fn execute_at_path(path: &Path) -> eyre::Result<Word> {
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
            let command = line.split_once(" ").map(|(c, _)| c).unwrap_or(line);
            let args = line.split(" ").skip(1).collect::<Vec<_>>();
            Ok(match (command, &args[..]) {
                ("PUSH", [v]) => OpCode::Push(v.parse()?),
                ("ADD", []) => OpCode::Add,
                ("DEBUG", []) => OpCode::Debug,
                ("EXIT", [v]) => OpCode::Exit(v.parse()?),

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
    Push(Word),

    Add,

    Exit(Word),

    Debug,
}

async fn execute(program: &Program) -> eyre::Result<Word> {
    let mut stack = Vec::<Word>::new();

    let pop = |s: &mut Vec<Word>| s.pop().ok_or_eyre("Pop from empty stack");

    for op in &program.ops {
        match op {
            OpCode::Push(w) => stack.push(*w),
            OpCode::Add => {
                let a = pop(&mut stack)?;
                let b = pop(&mut stack)?;
                stack.push(a + b);
            }

            OpCode::Exit(code) => return Ok(*code),

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
