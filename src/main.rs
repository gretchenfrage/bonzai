#![feature(fixed_size_array)]

extern crate core;

use core::array::FixedSizeArray;
use std::cell::{UnsafeCell, Cell};
use std::ops::Drop;
use std::marker::PhantomData;
use std::ptr;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ChildId {
    index: Option<usize>
}

#[derive(Debug, Copy, Clone)]
pub struct InvalidBranchIndex(pub usize);

enum Node<T, C: FixedSizeArray<ChildId>> {
    Garbage,
    Present {
        elem: UnsafeCell<T>,
        parent: Cell<ParentId>,
        children: UnsafeCell<C>
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum ParentId {
    Some(usize),
    Root,
    Detached,
}

pub struct Tree<T, C: FixedSizeArray<ChildId>> {
    nodes: Vec<UnsafeCell<Node<T, C>>>,
    root: Option<usize>,
    garbage: Vec<usize>,
}
impl<T, C: FixedSizeArray<ChildId>> Tree<T, C> {
    pub fn new() -> Self {
        Tree {
            nodes: Vec::new(),
            root: None,
            garbage: Vec::new()
        }
    }

    pub fn write_root<'tree>(&'tree mut self) -> Option<NodeWriteGuard<'tree, 'tree, T, C>> {
        unimplemented!()
    }

    pub fn take_root<'tree>(&'tree mut self) -> Option<NodeOwnedGuard<'tree, T, C>> {
        unimplemented!()
    }

    pub fn put_root_elem(&mut self, elem: T) {
        unimplemented!()
    }

    pub fn put_root_tree<'tree>(&'tree mut self, tree: NodeOwnedGuard<'tree, T, C>) {
        unimplemented!()
    }

    pub fn garbage_collect(&mut self) {
        unimplemented!()
    }
}
unsafe impl<T: Send, C: FixedSizeArray<ChildId>> Send for Tree<T, C> {}
unsafe impl<T: Sync, C: FixedSizeArray<ChildId>> Sync for Tree<T, C> {}

pub struct NodeWriteGuard<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree mut Tree<T, C>,
    index: usize,

    p1: PhantomData<&'node mut ()>,
}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> NodeWriteGuard<'tree, 'node, T, C> {
    pub fn split<'child>(&'child mut self) -> (&'child mut T, ChildGuard<'tree, 'child, T, C>) {
        unsafe {
            if let &Node::Present {
                ref elem,
                ref children,
                ..
            } = &*self.tree.nodes[self.index].get() {
                let elem: &'child mut T = &mut*elem.get();
                let children: &'child mut C = &mut*children.get();
                let child_guard: ChildGuard<'tree, 'child, T, C> = ChildGuard {
                    tree: self.tree,
                    children
                };
                (elem, child_guard)
            } else {
                unreachable!("guarding garbage")
            }
        }
    }
}

pub struct NodeOwnedGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree mut Tree<T, C>,
    index: usize,
    reattached: bool,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> NodeOwnedGuard<'tree, T, C> {
    pub fn split<'child>(&'child mut self) -> (&'child mut T, ChildGuard<'tree, 'child, T, C>) {
        unsafe {
            if let &Node::Present {
                ref elem,
                ref children,
                ..
            } = &*self.tree.nodes[self.index].get() {
                let elem: &'child mut T = &mut*elem.get();
                let children: &'child mut C = &mut*children.get();
                let child_guard: ChildGuard<'tree, 'child, T, C> = ChildGuard {
                    tree: self.tree,
                    children
                };
                (elem, child_guard)
            } else {
                unreachable!("write-guarding garbage")
            }
        }
    }
}
impl<'tree, T, C: FixedSizeArray<ChildId>> Drop for NodeOwnedGuard<'tree, T, C> {
    fn drop(&mut self) {
        unimplemented!()
    }
}

pub struct ChildGuard<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree mut Tree<T, C>,
    children: &'node mut C,
}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> ChildGuard<'tree, 'node, T, C> {
    pub fn write_child<'child>(&'child mut self, branch: usize)
        -> Result<Option<NodeWriteGuard<'tree, 'child, T, C>>, InvalidBranchIndex> {

        self.children.as_slice().get(branch)
            .ok_or(InvalidBranchIndex(branch))
            .map(|child_id| child_id.index)
            .map(|child_index| child_index
                .map(move |child_index| NodeWriteGuard {
                    tree: self.tree,
                    index: child_index,

                    p1: PhantomData
                }))
    }

    pub fn take_child(&mut self, branch: usize)
        -> Result<Option<NodeOwnedGuard<'tree, T, C>>, InvalidBranchIndex> {

        self.children.as_slice().get(branch)
            .ok_or(InvalidBranchIndex(branch))
            .map(|child_id| child_id.index)
            .map(|child_index| child_index
                .map(move |child_index| {
                    // detach the parent
                    unsafe {
                        if let &Node::Present {
                            ref parent,
                            ..
                        } = &*self.tree.nodes[child_index].get() {
                            parent.set(ParentId::Detached);
                        } else {
                            unreachable!("child index points to garbage");
                        }
                    }

                    // detach the child
                    self.children.as_mut_slice()[branch] = ChildId {
                        index: None
                    };

                    // create the guard
                    NodeOwnedGuard {
                        tree: self.tree,
                        index: child_index,
                        reattached: false,
                    }
                })
            )
    }

    pub fn put_child_elem(&mut self, branch: usize, elem: T) -> Result<bool, InvalidBranchIndex> {
        unimplemented!()
    }

    pub fn put_child_tree(&mut self, branch: usize, tree: NodeOwnedGuard<'tree, T, C>) -> Result<bool, InvalidBranchIndex> {
        unimplemented!()
    }
}

// TODO: read-only access

fn main() {
    println!("hello world");
}