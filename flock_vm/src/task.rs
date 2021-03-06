use flock_bytecode::{ByteCode, ConditionFlags, OpCode};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Task {
    pub(crate) program_counter: usize,
    pub(crate) stack: Vec<i64>,
    pub(crate) forked: bool,
}

impl Task {
    pub fn new() -> Task {
        Task {
            program_counter: 0,
            stack: Vec::new(),
            forked: false,
        }
    }

    pub fn run(&mut self, bytecode: &ByteCode) -> Result<Execution, ExecutionError> {
        loop {
            if let ControlFlow::Return(execution) = self.tick(bytecode)? {
                return Ok(execution);
            }
        }
    }

    fn tick(&mut self, bytecode: &ByteCode) -> Result<ControlFlow, ExecutionError> {
        let op = match bytecode.get(self.program_counter) {
            Some(op) => op,
            None => return Ok(ControlFlow::Return(Execution::Terminated)),
        };
        self.program_counter += 1;

        match op {
            OpCode::Push(value) => {
                self.stack.push(*value);
            }
            OpCode::Add => {
                let a = self.pop()?;
                let b = self.pop()?;
                self.stack.push(a.overflowing_add(b).0);
            }
            OpCode::DumpDebug => {
                self.print_debug(bytecode);
            }
            OpCode::Jump(flags, target) => {
                let target = match target {
                    None => self.pop()?,
                    Some(t) => *t,
                };

                let should_jump = {
                    let zero = flags
                        .contains(ConditionFlags::ZERO)
                        .implies(*self.peek()? == 0);
                    let forked = flags.contains(ConditionFlags::FORK).implies(self.forked);
                    zero && forked
                };
                if should_jump {
                    self.program_counter = target as usize;
                }
            }
            OpCode::JumpToSubroutine(target) => {
                let target = match target {
                    None => self.pop()?,
                    Some(t) => *t,
                };

                self.stack.push(self.program_counter as i64);
                self.program_counter = target as usize;
            }
            OpCode::Bury(index) => {
                let value = self.pop()?;

                let insert_index = self
                    .stack
                    .len()
                    .checked_sub(*index as usize)
                    .ok_or(ExecutionError::BuryOutOfRange(*index))?;

                self.stack.insert(insert_index, value);
            }
            OpCode::Dredge(index) => {
                let remove_index = (self.stack.len() - 1)
                    .checked_sub(*index as usize)
                    .ok_or(ExecutionError::DredgeOutOfRange(*index))?;
                let value = self.stack.remove(remove_index);
                self.stack.push(value);
            }
            OpCode::Duplicate => {
                let value = self.pop()?;
                self.stack.push(value);
                self.stack.push(value);
            }
            OpCode::Pop => {
                self.pop()?;
            }
            OpCode::Return => {
                let target = self.pop()?;
                self.program_counter = target as usize;
            }
            OpCode::Fork => {
                return Ok(ControlFlow::Return(Execution::Fork));
            }
            OpCode::Join(count) => {
                let task_id = self.pop()? as usize;
                return Ok(ControlFlow::Return(Execution::Join {
                    task_id,
                    count: *count as usize,
                }));
            }
            OpCode::Halt => {
                return Ok(ControlFlow::Return(Execution::Terminated));
            }
            OpCode::Store(addr) => {
                let value = self.pop()?;
                return Ok(ControlFlow::Return(Execution::Store { addr: *addr, value }));
            }
            OpCode::StoreRelative(base) => {
                let offset = self.pop()?;
                let addr = base.wrapping_add(offset as u64);
                let value = self.pop()?;
                return Ok(ControlFlow::Return(Execution::Store { addr, value }));
            }
            OpCode::Load(addr) => {
                return Ok(ControlFlow::Return(Execution::Load { addr: *addr }));
            }
            OpCode::LoadRelative(base) => {
                let offset = self.pop()?;
                let addr = base.wrapping_add(offset as u64);
                return Ok(ControlFlow::Return(Execution::Load { addr }));
            }
            OpCode::Panic => {
                return Err(ExecutionError::ExplicitPanic);
            }
            op => {
                unimplemented!("Unhandled opcode {:?}", op);
            }
        }

        Ok(ControlFlow::Continue)
    }

    fn pop(&mut self) -> Result<i64, ExecutionError> {
        self.stack.pop().ok_or(ExecutionError::PopFromEmptyStack)
    }

    fn peek(&mut self) -> Result<&i64, ExecutionError> {
        self.stack
            .get(self.stack.len() - 1)
            .ok_or(ExecutionError::PeekFromEmptyStack)
    }

    fn print_debug(&self, bytecode: &ByteCode) {
        eprintln!("Flock VM Debug");
        eprintln!("PC: {}", self.program_counter);

        eprintln!("");

        eprintln!("OpCodes:");
        let bounds: usize = 5;
        for (i, op) in bytecode.surrounding(self.program_counter, bounds) {
            let delta = (i as isize) - (self.program_counter as isize);
            eprintln!("  {:#2}: {:?}", delta, op);
        }

        eprintln!("");

        eprintln!("Stack:");
        for (i, value) in self.stack.iter().rev().enumerate() {
            eprintln!("  {:#03} {:#018x} ({})", i, value, value)
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum ExecutionError {
    PopFromEmptyStack,
    PeekFromEmptyStack,
    DredgeOutOfRange(i64),
    BuryOutOfRange(i64),
    UnknownTaskId(usize),
    UnableToProgress,
    ExplicitPanic,
}

impl std::error::Error for ExecutionError {}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub enum ControlFlow {
    Continue,
    Return(Execution),
}

#[derive(Debug)]
pub enum Execution {
    Terminated,
    Fork,
    Join { task_id: usize, count: usize },
    Store { addr: u64, value: i64 },
    Load { addr: u64 },
}

trait BoolImplies {
    fn implies(self, other: Self) -> Self;
}

impl BoolImplies for bool {
    fn implies(self, other: bool) -> bool {
        if self {
            other
        } else {
            true
        }
    }
}
