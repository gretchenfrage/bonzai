#![feature(fixed_size_array)]

extern crate core;

use core::array::FixedSizeArray;
use std::cell::{UnsafeCell, Cell};
use std::ops::{Deref, DerefMut};
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

//pub struct NodeOwnedGuard<>

pub struct ChildGuard<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree mut Tree<T, C>,
    children: &'node mut C,
}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> ChildGuard<'tree, 'node, T, C> {
    pub fn write_child<'child>(&'child mut self) -> Option<NodeWriteGuard<'tree, 'child, T, C>> {
        unimplemented!()
    }

    pub fn take_child(&mut self) -> Option<NodeOwnedGuard<'tree, T, C>> {
        unimplemented!()
    }
}

/*
struct Node<T, C: FixedSizeArray<ChildId>> {
    user_data: T,
    parent: Option<usize>,
    children: C
}

type NodeElem<T, C> = UnsafeCell<Option<Node<T, C>>>;

pub struct Tree<T, C: FixedSizeArray<ChildId>> {
    nodes: Vec<NodeElem<T, C>>,
    root: Option<usize>,
    garbage: Vec<usize>,
}

unsafe impl<T: Send, C: FixedSizeArray<ChildId>> Send for Tree<T, C> {}
unsafe impl<T: Sync, C: FixedSizeArray<ChildId>> Sync for Tree<T, C> {}

impl<T, C: FixedSizeArray<ChildId>> Tree<T, C> {
    pub fn new() -> Self {
        Tree {
            nodes: Vec::new(),
            root: None,
            garbage: Vec::new(),
        }
    }

    //pub fn garbage_collect(&mut self) {
    //    unimplemented!()
    //}
//
    //pub fn read_root(&'a self) -> Option<>
}

pub struct NodeGuard<'tree, 'node, T, C: FixedSizeArray<ChildId>> {

}

/*
pub struct NodeReadGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    node: &'tree Node<T, C>,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> Deref for NodeReadGuard<'tree, T, C> {
    type Target = T;

    fn deref(&self) -> &<Self as Deref>::Target {
        unsafe {
            &(&*self.node_pointer).user_data
        }
    }
}
impl<'tree, T, C: FixedSizeArray<ChildId>> NodeReadGuard<'tree, T, C> {
    pub fn new(tree: &'tree Tree<T, C>, index: usize) -> Self {
        let mut guard = NodeReadGuard {
            tree,
            node: unsafe {
                (&*tree.nodes[index].get()).as_ref().unwrap()
            },
        };
    }

    pub fn child(&self, branch: usize) -> Option<NodeReadGuard<'tree, T, C>> {
        unimplemented!()
    }
}

pub struct NodeWriteGuard<'tree, 'node, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree mut Tree<T, C>,
    index: usize,
    node_pointer: *mut Node<T, C>,
    node_lifetime: PhantomData<&'node mut ()>,
}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> Deref for NodeWriteGuard<'tree, 'node, T, C> {
    type Target = T;

    fn deref(&self) -> &<Self as Deref>::Target {
        unsafe {
            &(&*self.node_pointer).user_data
        }
    }
}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> DerefMut for NodeWriteGuard<'tree, 'node, T, C> {
    fn deref_mut(&mut self) -> &mut <Self as Deref>::Target {
        unsafe {
            &mut(&mut*self.node_pointer).user_data
        }
    }
}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> NodeWriteGuard<'tree, 'node, T, C> {
    fn new(tree: &'tree mut Tree<T, C>, index: usize) -> Self {
        let mut guard = NodeWriteGuard {
            tree,
            index,
            node_pointer: ptr::null_mut(),
            node_lifetime: PhantomData,
        };
        guard.refresh_pointer();
        guard
    }

    fn refresh_pointer(&mut self) {
        unsafe {
            self.node_pointer = (&mut*self.tree.nodes[self.index].get()).as_mut().unwrap();
        }
    }

    // TODO: ideally, we'd have the node write guard be able to guard the lack of a node
    // TODO: and it could replace itself

    how to update a node and, in doing so, access its children

    pub fn read_child<'s>(&'s self, branch: usize) -> Option<NodeReadGuard<'s, T, C>> {
        unimplemented!()
    }

    pub fn child<'s>(&'s mut self, branch: usize) -> Option<NodeWriteGuard<'tree, 's, T, C>> {
        unimplemented!()
    }

    pub fn replace_child(&mut self, branch: usize, replacment: Option<T>) -> Option<T> {
        unimplemented!()
    }

    pub fn update_child(&mut self, branch: usize, updater: impl FnOnce(Option<T>) -> Option<T>) {
        unimplemented!()
    }

    pub fn update_derive_child<O>(&mut self, branch: usize,
                                  updater: impl FnOnce(Option<T>) -> (Option<T>, O)) -> O {
        unimplemented!()
    }
}
*/
*/

fn main() {
    println!("hello world");
}