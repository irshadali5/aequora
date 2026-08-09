//! Server-owned policy expressions for opaque partial-synchronization partitions.

use aequora_protocol::Partition;
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

/// Boolean policy expression evaluated against client-requested partition selectors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PartitionExpression {
    /// Matches every structurally valid requested scope.
    Any,
    /// Requires this exact selector.
    Exact(Partition),
    /// Requires at least one selector with this application-defined kind.
    Kind(u16),
    /// Requires a selector equal to or below this hierarchy node.
    DescendantOf(Partition),
    /// Every child expression must match.
    All(Vec<Self>),
    /// At least one child expression must match.
    AnyOf(Vec<Self>),
    /// The child expression must not match.
    Not(Box<Self>),
}

/// Invalid server-side partition policy configuration.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PartitionPolicyError {
    /// The expression exceeds its bounded node count.
    #[error("partition expression exceeds {maximum} nodes")]
    TooComplex { maximum: usize },
    /// A hierarchy edge would introduce a cycle.
    #[error("partition hierarchy must remain acyclic")]
    HierarchyCycle,
}

/// Acyclic parent/child relationships between otherwise opaque selectors.
#[derive(Clone, Debug, Default)]
pub struct PartitionHierarchy {
    children: HashMap<Partition, HashSet<Partition>>,
}

impl PartitionHierarchy {
    /// Adds a parent-child edge after proving it cannot create a cycle.
    ///
    /// # Errors
    ///
    /// Returns [`PartitionPolicyError::HierarchyCycle`] for self-links or cycles.
    pub fn add_child(
        &mut self,
        parent: Partition,
        child: Partition,
    ) -> Result<(), PartitionPolicyError> {
        if parent == child || self.is_descendant(&child, &parent) {
            return Err(PartitionPolicyError::HierarchyCycle);
        }
        self.children.entry(parent).or_default().insert(child);
        Ok(())
    }

    /// Returns whether `candidate` is the same node as `ancestor` or a transitive child.
    #[must_use]
    pub fn is_descendant(&self, ancestor: &Partition, candidate: &Partition) -> bool {
        if ancestor == candidate {
            return true;
        }
        let mut pending = VecDeque::from([ancestor]);
        let mut visited = HashSet::new();
        while let Some(node) = pending.pop_front() {
            if !visited.insert(node) {
                continue;
            }
            if let Some(children) = self.children.get(node) {
                if children.contains(candidate) {
                    return true;
                }
                pending.extend(children);
            }
        }
        false
    }
}

/// Validated bounded expression and optional application hierarchy.
#[derive(Clone, Debug)]
pub struct PartitionPolicy {
    expression: PartitionExpression,
    hierarchy: PartitionHierarchy,
}

impl PartitionPolicy {
    /// Validates a server-owned expression before it can authorize client scopes.
    ///
    /// # Errors
    ///
    /// Returns [`PartitionPolicyError::TooComplex`] when recursive configuration exceeds
    /// `maximum_nodes`.
    pub fn new(
        expression: PartitionExpression,
        hierarchy: PartitionHierarchy,
        maximum_nodes: usize,
    ) -> Result<Self, PartitionPolicyError> {
        if expression_nodes(&expression) > maximum_nodes {
            return Err(PartitionPolicyError::TooComplex {
                maximum: maximum_nodes,
            });
        }
        Ok(Self {
            expression,
            hierarchy,
        })
    }

    /// Evaluates this server-owned policy against the client's requested selectors.
    #[must_use]
    pub fn allows(&self, requested: &[Partition]) -> bool {
        matches_expression(&self.expression, requested, &self.hierarchy)
    }
}

fn expression_nodes(expression: &PartitionExpression) -> usize {
    match expression {
        PartitionExpression::All(children) | PartitionExpression::AnyOf(children) => {
            children.iter().fold(1_usize, |count, child| {
                count.saturating_add(expression_nodes(child))
            })
        }
        PartitionExpression::Not(child) => 1_usize.saturating_add(expression_nodes(child)),
        _ => 1,
    }
}

fn matches_expression(
    expression: &PartitionExpression,
    requested: &[Partition],
    hierarchy: &PartitionHierarchy,
) -> bool {
    match expression {
        PartitionExpression::Any => true,
        PartitionExpression::Exact(expected) => requested.contains(expected),
        PartitionExpression::Kind(kind) => requested.iter().any(|value| value.kind == *kind),
        PartitionExpression::DescendantOf(ancestor) => requested
            .iter()
            .any(|candidate| hierarchy.is_descendant(ancestor, candidate)),
        PartitionExpression::All(children) => children
            .iter()
            .all(|child| matches_expression(child, requested, hierarchy)),
        PartitionExpression::AnyOf(children) => children
            .iter()
            .any(|child| matches_expression(child, requested, hierarchy)),
        PartitionExpression::Not(child) => !matches_expression(child, requested, hierarchy),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partition(kind: u16, value: u8) -> Partition {
        Partition {
            kind,
            value: vec![value],
        }
    }

    #[test]
    fn hierarchy_and_boolean_rules_authorize_only_the_intended_scope() {
        let school = partition(1, 1);
        let class = partition(2, 8);
        let forbidden = partition(3, 9);
        let mut hierarchy = PartitionHierarchy::default();
        hierarchy
            .add_child(school.clone(), class.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let policy = PartitionPolicy::new(
            PartitionExpression::All(vec![
                PartitionExpression::DescendantOf(school),
                PartitionExpression::Not(Box::new(PartitionExpression::Exact(forbidden.clone()))),
            ]),
            hierarchy,
            8,
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert!(policy.allows(std::slice::from_ref(&class)));
        assert!(!policy.allows(&[class, forbidden]));
    }
}
