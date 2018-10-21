#![feature(fixed_size_array)]
#![feature(optin_builtin_traits)]
#![allow(mutable_transmutes)]

extern crate core;

use core::array::FixedSizeArray;
use std::cell::{UnsafeCell, Cell};
use std::ops::Drop;
use std::marker::PhantomData;
use std::ptr;
use std::mem;
use std::fmt::{Debug, Formatter};
use std::fmt;

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

fn new_child_array<C: FixedSizeArray<ChildId>>() -> C {
    unsafe {
        let mut children: C = mem::uninitialized();
        for child_ref in children.as_mut_slice() {
            ptr::write(child_ref, ChildId {
                index: None
            });
        }
        children
    }
}

pub struct Tree<T, C: FixedSizeArray<ChildId>> {
    nodes: UnsafeCell<Vec<UnsafeCell<Node<T, C>>>>,
    root: Option<usize>,
    garbage: UnsafeCell<Vec<usize>>,
}
impl<T, C: FixedSizeArray<ChildId>> Tree<T, C> {
    pub fn new() -> Self {
        Tree {
            nodes: UnsafeCell::new(Vec::new()),
            root: None,
            garbage: UnsafeCell::new(Vec::new()),
        }
    }

    pub fn write_root<'tree>(&'tree mut self) -> Option<NodeWriteGuard<'tree, 'tree, T, C>> {
        let self_immutable: &Self = self;

        self_immutable.root
            .map(|root_index| NodeWriteGuard {
                tree: self_immutable,
                index: root_index,

                p1: PhantomData,
                p2: PhantomData,
            })
    }

    pub fn take_root<'tree>(&'tree mut self) -> Option<NodeOwnedGuard<'tree, T, C>> {
        self.root
            .map(move |root_index| {
                // detach the parent
                unsafe {
                    if let &Node::Present {
                        ref parent,
                        ..
                    } = &*(&*self.nodes.get())[root_index].get() {
                        debug_assert_eq!(parent.get(), ParentId::Root);
                        parent.set(ParentId::Detached);
                    } else {
                        unreachable!("root index points to garbage");
                    }
                }

                // detach the root
                self.root = None;

                // create the guard
                NodeOwnedGuard {
                    tree: self,
                    index: root_index,
                    reattached: false,

                    p1: PhantomData,
                }
            })
    }

    unsafe fn delete_root(&mut self, nodes_vec: &mut Vec<UnsafeCell<Node<T, C>>>) -> bool {
        if let Some(former_root_index) = self.root {
            *(&mut*nodes_vec[former_root_index].get()) = Node::Garbage;
            (&mut*self.garbage.get()).push(former_root_index);
            true
        } else {
            false
        }
    }

    pub fn put_root_elem(&mut self, elem: T) -> bool {
        unsafe {
            // unsafely create the new children array
            let child_children: C = new_child_array();

            // create the new node
            let child_node = Node::Present {
                elem: UnsafeCell::new(elem),
                parent: Cell::new(ParentId::Root),
                children: UnsafeCell::new(child_children),
            };

            let nodes_vec = &mut*self.nodes.get();

            // insert it into the nodes vector, get the index
            nodes_vec.push(UnsafeCell::new(child_node));
            let child_index = nodes_vec.len() - 1;

            // mark any existing root as garbage
            let deleted = self.delete_root(nodes_vec);

            // attach the root
            self.root = Some(child_index);

            // done
            deleted
        }
    }

    pub fn put_root_tree<'tree>(&'tree mut self, mut subtree: NodeOwnedGuard<'tree, T, C>) -> bool {
        unsafe {
            let nodes_vec = &mut*self.nodes.get();

            // mark any existing root as garbage
            let deleted = self.delete_root(nodes_vec);

            // attach the root
            self.root = Some(subtree.index);

            // attach the parent
            if let &Node::Present {
                ref parent,
                ..
            } = &*nodes_vec[subtree.index].get() {
                debug_assert_eq!(parent.get(), ParentId::Detached);
                parent.set(ParentId::Root);
            } else {
                unreachable!("put root tree references garbage");
            }

            // drop the NodeOwnedGuard without triggering it to mark the node as garbage
            subtree.reattached = true;
            mem::drop(subtree);

            // done
            deleted
        }
    }

    pub fn garbage_collect(&mut self) {
        unimplemented!()
    }
}
unsafe impl<T: Send, C: FixedSizeArray<ChildId>> Send for Tree<T, C> {}
unsafe impl<T: Sync, C: FixedSizeArray<ChildId>> Sync for Tree<T, C> {}
impl<T: Debug, C: FixedSizeArray<ChildId>> Debug for Tree<T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        // TODO: bad and wrong
        unsafe { mem::transmute::<&Self, &mut Self>(self).write_root().fmt(f) }
    }
}

pub struct NodeWriteGuard<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    index: usize,

    p1: PhantomData<&'node mut ()>,
    p2: PhantomData<&'tree mut ()>,
}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> NodeWriteGuard<'tree, 'node, T, C> {
    pub fn split<'child>(&'child mut self) -> (&'child mut T, ChildGuard<'tree, 'child, T, C>) {
        unsafe {
            if let &Node::Present {
                ref elem,
                ref children,
                ..
            } = &*(&*self.tree.nodes.get())[self.index].get() {
                let elem: &'child mut T = &mut*elem.get();
                let children: &'child mut C = &mut*children.get();
                let child_guard: ChildGuard<'tree, 'child, T, C> = ChildGuard {
                    tree: self.tree,
                    index: self.index,
                    children,

                    p1: PhantomData,
                };
                (elem, child_guard)
            } else {
                unreachable!("guarding garbage")
            }
        }
    }
}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> !Send for NodeWriteGuard<'tree, 'node, T, C> {}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> !Sync for NodeWriteGuard<'tree, 'node, T, C> {}
impl<'tree, 'node: 'tree, T: Debug, C: FixedSizeArray<ChildId>> Debug for NodeWriteGuard<'tree, 'node, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        // TODO: don't transmute imut to mut, that's bad and wrong and illegal
        // TODO: but in this case, it should work for now
        let (elem, mut children) = unsafe { mem::transmute::<&Self, &mut Self>(self).split() };
        let mut builder = f.debug_struct("Node");
        builder.field("elem", elem);
        let num_children = children.children.as_slice().len();
        for b in 0..num_children {
            let child = children.write_child(b).unwrap();
            builder.field(&format!("child_{}", b), &child);
        }
        builder.finish()
    }
}

pub struct NodeOwnedGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    index: usize,
    reattached: bool,

    p1: PhantomData<&'tree mut ()>,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> NodeOwnedGuard<'tree, T, C> {
    pub fn split<'child>(&'child mut self) -> (&'child mut T, ChildGuard<'tree, 'child, T, C>) {
        unsafe {
            if let &Node::Present {
                ref elem,
                ref children,
                ..
            } = &*(&*self.tree.nodes.get())[self.index].get() {
                let elem: &'child mut T = &mut*elem.get();
                let children: &'child mut C = &mut*children.get();
                let child_guard: ChildGuard<'tree, 'child, T, C> = ChildGuard {
                    tree: self.tree,
                    index: self.index,
                    children,

                    p1: PhantomData,
                };
                (elem, child_guard)
            } else {
                unreachable!("write-guarding garbage")
            }
        }
    }

    // TODO: take elem
}
impl<'tree, T, C: FixedSizeArray<ChildId>> Drop for NodeOwnedGuard<'tree, T, C> {
    fn drop(&mut self) {
        if !self.reattached {
            unsafe {
                *(&mut*((&(&*(self.tree.nodes.get()))[self.index]).get())) = Node::Garbage;
            }
        }
    }
}

pub struct ChildGuard<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    index: usize,
    children: &'node mut C,

    p1: PhantomData<&'tree mut ()>,
}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> ChildGuard<'tree, 'node, T, C> {
    pub fn write_child<'this, 'child: 'this>(&'this mut self, branch: usize)
        -> Result<Option<NodeWriteGuard<'tree, 'child, T, C>>, InvalidBranchIndex> {

        self.children.as_slice().get(branch)
            .ok_or(InvalidBranchIndex(branch))
            .map(|child_id| child_id.index)
            .map(|child_index| child_index
                .map(move |child_index| NodeWriteGuard {
                    tree: self.tree,
                    index: child_index,

                    p1: PhantomData,
                    p2: PhantomData,
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
                        } = &*(&*self.tree.nodes.get())[child_index].get() {
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

                        p1: PhantomData
                    }
                })
            )
    }

    unsafe fn delete_child(&mut self,
                           nodes_vec: &mut Vec<UnsafeCell<Node<T, C>>>,
                           branch: usize) -> bool {
        if let ChildId {
            index: Some(former_child_index)
        } = self.children.as_slice()[branch] {
            *(&mut*nodes_vec[former_child_index].get()) = Node::Garbage;
            (&mut*self.tree.garbage.get()).push(former_child_index);
            true
        } else {
            false
        }
    }

    pub fn put_child_elem(&mut self, branch: usize, elem: T) -> Result<bool, InvalidBranchIndex> {
        unsafe {
            // short-circuit if the branch is invalid
            if branch >= self.children.as_slice().len() {
                return Err(InvalidBranchIndex(branch));
            }

            // unsafely create the new children array
            let child_children: C = new_child_array();

            // create the new node
            let child_node = Node::Present {
                elem: UnsafeCell::new(elem),
                parent: Cell::new(ParentId::Some(self.index)),
                children: UnsafeCell::new(child_children)
            };

            let nodes_vec = &mut*self.tree.nodes.get();

            // insert it into the nodes vector, get the index
            nodes_vec.push(UnsafeCell::new(child_node));
            let child_index = nodes_vec.len() - 1;

            // mark any existing child as garbage
            let deleted = self.delete_child(nodes_vec, branch);

            // attach the child
            self.children.as_mut_slice()[branch] = ChildId {
                index: Some(child_index)
            };

            // done
            Ok(deleted)
        }
    }

    pub fn put_child_tree(&mut self, branch: usize, mut subtree: NodeOwnedGuard<'tree, T, C>) -> Result<bool, InvalidBranchIndex> {
        unsafe {
            // short-circuit if the branch is invalid
            if branch >= self.children.as_slice().len() {
                return Err(InvalidBranchIndex(branch));
            }

            let nodes_vec = &mut*self.tree.nodes.get();

            // mark any existing child as garbage
            let deleted = self.delete_child(nodes_vec, branch);

            // attach the child
            self.children.as_mut_slice()[branch] = ChildId {
                index: Some(subtree.index),
            };

            // attach the parent
            if let &Node::Present {
                ref parent,
                ..
            } = &*nodes_vec[subtree.index].get() {
                debug_assert_eq!(parent.get(), ParentId::Detached);
                parent.set(ParentId::Some(self.index));
            } else {
                unreachable!("put child tree references garbage");
            }

            // drop the NodeOwnedGuard without triggering it to mark the node as garbage
            subtree.reattached = true;
            mem::drop(subtree);

            // done
            Ok(deleted)
        }
    }
}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> !Send for ChildGuard<'tree, 'node, T, C> {}
impl<'tree, 'node: 'tree, T, C: FixedSizeArray<ChildId>> !Sync for ChildGuard<'tree, 'node, T, C> {}

// TODO: read-only access
// TODO: creation of a detached node which was never attached
// TODO: garbage collection

fn main() {
    let mut tree: Tree<i32, [ChildId; 2]> = Tree::new();
    println!("{:#?}", tree);
    println!("{:#?}", tree.write_root());
    println!("{}", tree.put_root_elem(0));
    println!("{:#?}", tree);
    {
        let mut root_guard = tree.write_root().unwrap();
        let (mut root, mut root_children) = root_guard.split();
        println!("{}", root);
        *root += 1;
        println!("{:?}", root_children.write_child(0));
        println!("{:?}", root_children.write_child(1));
        println!("{:?}", root_children.write_child(2));
        println!("-a");
        println!("{:?}", root_children.put_child_elem(0, 2));
        println!("{:?}", root_children.put_child_elem(1, 3));
        println!("{:?}", root_children.put_child_elem(0, 4));
        println!("{:?}", root_children.put_child_elem(2, 7));
    }
    println!("{:#?}", tree);
    {
        let mut root_guard = tree.write_root().unwrap();
        let (mut root, mut root_children) = root_guard.split();
        let detached = root_children.take_child(1).unwrap().unwrap();
        {
            let mut node_0_guard = root_children.write_child(0).unwrap().unwrap();
            let (mut node_0, mut node_0_children) = node_0_guard.split();
            node_0_children.put_child_tree(1, detached);
            *node_0 = 42;
        }
    }
    println!("{:#?}", tree);
}