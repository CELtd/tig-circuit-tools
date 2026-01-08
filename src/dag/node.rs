/// Operation types in the circuit DAG
#[derive(Debug, Clone, PartialEq)]
pub enum OpType {
    /// Placeholder during DAG construction
    Undefined,
    /// Public input variable
    Input,
    /// Public output variable
    Output,
    /// Addition: out = left + right (1 constraint)
    Add(usize, usize),
    /// Multiplication: out = left * right (1 constraint, non-linear)
    Mul(usize, usize),
    /// Alias: out = source (1 constraint, optimization trap)
    Alias(usize),
    /// Scaling: out = k * source (1 constraint, linear trap)
    Scale(usize, u64),
    /// Fifth power: out = source^5 (3 constraints, algebraic trap)
    Pow5(usize),
}

/// A node in the circuit DAG
#[derive(Debug, Clone)]
pub struct Node {
    /// Unique identifier for this node
    pub id: usize,
    /// The operation this node performs
    pub op: OpType,
}

impl Node {
    /// Creates a new node with the given ID and operation
    pub fn new(id: usize, op: OpType) -> Self {
        Self { id, op }
    }

    /// Returns the number of constraints this node contributes
    pub fn constraint_count(&self) -> usize {
        match self.op {
            OpType::Pow5(_) => 3,
            OpType::Add(_, _) | OpType::Mul(_, _) | OpType::Alias(_) | OpType::Scale(_, _) => 1,
            OpType::Undefined | OpType::Input | OpType::Output => 0,
        }
    }

    /// Returns true if this node is an input
    pub fn is_input(&self) -> bool {
        matches!(self.op, OpType::Input)
    }

    /// Returns true if this node is an output
    pub fn is_output(&self) -> bool {
        matches!(self.op, OpType::Output)
    }
}
