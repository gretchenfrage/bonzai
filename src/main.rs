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
pub struct InvalidChildIndex(pub usize);

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
                unreachable!("guarding garbage")
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
        -> Result<Option<NodeWriteGuard<'tree, 'child, T, C>>, InvalidChildIndex> {

        unimplemented!()
    }

    pub fn take_child(&mut self, branch: usize)
        -> Result<Option<NodeOwnedGuard<'tree, T, C>>, InvalidChildIndex> {

        unimplemented!()
    }

    pub fn put_child_elem(&mut self, branch: usize, elem: T) -> Result<bool, InvalidChildIndex> {
        unimplemented!()
    }

    pub fn put_child_tree(&mut self, branch: usize, tree: NodeOwnedGuard<'tree, T, C>) -> Result<bool, InvalidChildIndex> {
        unimplemented!()
    }
}

fn main() {
    println!("hello world");
}