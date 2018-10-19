
#![feature(fixed_size_array)]

extern crate core;

use core::array::FixedSizeArray;
use std::cell::UnsafeCell;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ChildId {
    index: Option<usize>
}

struct Node<T, C: FixedSizeArray<ChildId>> {
    data: UnsafeCell<T>,
    parent: Option<usize>,
    children: C,
}

/// A tree, stored in a constant number of contiguous memory blocks, in order to increase
/// cache performance. Generic over element type and branch factor.
pub struct Tree<T, C: FixedSizeArray<ChildId>> {
    nodes: Vec<Option<Node<T, C>>>,
    root: Option<usize>,
    garbage: Vec<usize>,
}
impl<T, C: FixedSizeArray<ChildId>> Tree<T, C> {
    /// Construct a new, empty tree.
    pub fn new() -> Self {
        Tree {
            nodes: Vec::new(),
            root: None,
            garbage: Vec::new(),
        }
    }

    /// Remove nodes which are marked as garbage, which requires changing indices to maintain
    /// the integrity of parent/child relationships.
    pub fn compact(&mut self) {
        unimplemented!()
    }

    pub fn root(&self) -> OptionNodeGuard<T, C> {
        unimplemented!()
    }

}

/// A guard for immutably accessing a node and its children, even if the node does not exist.
pub struct OptionNodeGuard<'tree, 'node, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    node: Option<&'node Node<T, C>>,
}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> OptionNodeGuard<'tree, 'node, T, C> {
    pub fn as_option(&self) -> Option<NodeGuard<'tree, 'node, T, C>> {
        self.node.map(|node| NodeGuard {
            tree: self.tree,
            node
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub struct InvalidChildIndex(pub usize);

/// A guard for immutably accessing a node and its children.
pub struct NodeGuard<'tree, 'node, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    node: &'node Node<T, C>,
}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> NodeGuard<'tree, 'node, T, C> {
    /// Get the tree this node belongs to.
    pub fn tree(&self) -> &'tree Tree<T, C> {
        self.tree
    }

    /// Get this node's data.
    pub fn data(&self) -> &'node T {
        unsafe {
            &*self.node.data.get()
        }
    }

    /// Get a child of this node in the form of an `OptionNodeGuard`.
    pub fn child<'s>(&self, index: usize) -> Result<OptionNodeGuard<'tree, 's, T, C>, InvalidChildIndex> {
        self.node.children.as_slice()
            .get(index)
            .ok_or(InvalidChildIndex(index))
            .map(|child_id| OptionNodeGuard {
                tree: self.tree,
                node: child_id.index
                    .map(|i| (&self.tree.nodes[i])
                        .as_ref()
                        .unwrap())
            })
    }
}

/// A guard for mutably accessing a node and its children, even if the node does not exist.
pub struct OptionNodeGuardMut<'a, T, C: FixedSizeArray<ChildId>> {
    tree: &'a mut Tree<T, C>,
    node: Option<&'a mut Node<T, C>>
}
impl<'a, T, C: FixedSizeArray<ChildId>> OptionNodeGuardMut<'a, T, C> {
    pub fn as_option(&mut self) -> Option<NodeGuardMut<'a, T, C>> {
        self.node.map(|node| NodeGuardMut {
            tree: self.tree,
            node
        })
    }
}

/// A guard for mutable accessing a node and its children.
pub struct NodeGuardMut<'a, T, C: FixedSizeArray<ChildId>> {
    tree: &'a mut Tree<T, C>,
    node: &'a mut Node<T, C>
}
impl<'a, T, C: FixedSizeArray<ChildId>> NodeGuardMut<'a, T, C> {
    /// Get the tree this node belongs to.
    pub fn tree(&mut self) -> &mut Tree<T, C> {
        self.tree
    }

    /// Get this node's data.
    pub fn data(&mut self) -> &mut T {
        unsafe {
            &mut*self.node.data.get()
        }
    }

    /// Get a child of this node in the form of an `OptionNodeGuardMut`.
    pub fn child(&mut self, index: usize) -> OptionNodeGuardMut<'a, T, C> {
        self.node.children.as_slice()
            .get(index)
            .ok_or(InvalidChildIndex(index))
            .map(|child_id| OptionNodeGuard {
                tree: self.tree,
                node: child_id.index
                    .map(|i| (&mut self.tree.nodes[i])
                        .as_ref_mut()
                        .unwrap())
            })
    }
}


// TODO: if a node is marked as garbage, then all its children should be garbage collected
// TODO: implement debug

fn main() {
    println!("Hello, world!");
}
