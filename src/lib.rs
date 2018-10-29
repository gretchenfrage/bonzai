#![feature(fixed_size_array)]
#![feature(optin_builtin_traits)]

extern crate core;


//pub mod bst;
mod pinned_vec;

use pinned_vec::PinnedVec;

use core::array::FixedSizeArray;
use std::cell::{UnsafeCell, Cell};
use std::ops::{Deref, DerefMut, Drop};
use std::marker::PhantomData;
use std::ptr;
use std::mem;
use std::fmt::{Debug, Formatter};
use std::fmt;

const EXTENSION_SIZE: usize = 6;

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct ChildId {
    index: Option<usize>
}
impl Debug for ChildId {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        f.write_str(&format!("{:?}", self.index))
    }
}

#[derive(Debug, Copy, Clone)]
pub struct InvalidBranchIndex(pub usize);

enum Node<T, C: FixedSizeArray<ChildId>> {
    Garbage {
        children: C,
    },
    Present {
        elem: UnsafeCell<T>,
        parent: Cell<ParentId>,
        children: UnsafeCell<C>
    }
}
impl<T, C: FixedSizeArray<ChildId>> Node<T, C> {
    fn take_elem_become_garbage(&mut self) -> T {
        unsafe {
            let this = ptr::read(self);
            let (this, elem) = match this {
                Node::Present {
                    elem,
                    children,
                    ..
                } => (Node::Garbage {
                    children: children.into_inner()
                }, elem.into_inner()),
                Node::Garbage {
                    ..
                } => {
                    unreachable!("node become garbage, node already is garbage");
                },
            };
            ptr::write(self, this);
            elem
        }
    }
}
impl<T: Debug, C: FixedSizeArray<ChildId> + Debug> Debug for Node<T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        match self {
            &Node::Garbage { .. } => {
                f.debug_struct("Garbage")
                    .finish()
            },
            &Node::Present {
                ref elem,
                ref parent,
                ref children,
            } => unsafe {
                f.debug_struct("Node")
                    .field("elem", &*elem.get())
                    .field("parent", &parent.get())
                    .field("children", &*children.get())
                    .finish()
            }
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum ParentId {
    Some {
        parent_index: usize,
        this_branch: usize,
    },
    Root,
    Detached,
    Garbage,
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

pub struct DebugNodes<'a, T, C: FixedSizeArray<ChildId>> {
    nodes: &'a UnsafeCell<PinnedVec<UnsafeCell<Node<T, C>>>>,
}
impl<'a, T: Debug, C: FixedSizeArray<ChildId> + Debug> Debug for DebugNodes<'a, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let mut builder = f.debug_struct("Nodes");
        unsafe {
            for (i, node) in (&*self.nodes.get()).iter().enumerate() {
                builder.field(&format!("{}", i), &*node.get());
            }
        }
        builder.finish()
    }
}

pub struct Tree<T, C: FixedSizeArray<ChildId>> {
    nodes: UnsafeCell<PinnedVec<UnsafeCell<Node<T, C>>>>,
    root: Cell<Option<usize>>,
    garbage: UnsafeCell<Vec<usize>>,
}
impl<T, C: FixedSizeArray<ChildId>> Tree<T, C> {
    pub fn new() -> Self {
        Tree {
            nodes: UnsafeCell::new(PinnedVec::new(EXTENSION_SIZE)),
            root: Cell::new(None),
            garbage: UnsafeCell::new(Vec::new()),
        }
    }

    pub fn debug_nodes(&self) -> DebugNodes<T, C> {
        DebugNodes {
            nodes: &self.nodes
        }
    }

    pub fn read_root<'tree>(&'tree self) -> Option<NodeReadGuard<'tree, T, C>> {
        self.root.get()
            .map(|root_index| unsafe {
                NodeReadGuard::new(self, root_index)
            })
    }

    pub fn operation<'tree>(&'tree mut self) -> TreeOperation<'tree, T, C> {
        TreeOperation {
            tree: self
        }
    }

    pub fn health_check(&self) -> bool {
        true
        //unsafe {
        //    let mut health = true;
        //    match self.root.get() {
        //        Some(root_index) => {
        //
        //        },
        //        None => {
        //            if (&*self.nodes.get()).len() != 0{
        //                eprintln!("tree has no root, but there are nodes");
        //                healthy = false;
        //            }
        //        }
        //    }
        //    health
        //}
    }

    pub fn garbage_collect(&mut self) {
        unsafe {
            let garbage_vec = &mut*self.garbage.get();
            let nodes = &mut*self.nodes.get();

            nodes.compress();

            while let Some(garbage_index) = garbage_vec.pop() {
                if garbage_index >= nodes.len() {
                    continue;
                }

                debug_assert!(match &*(&nodes[garbage_index]).get() {
                    &Node::Garbage { .. } => true,
                    &Node::Present { .. } => false
                });

                // TODO: the children are also garbage

                let removed_node = nodes.swap_remove(garbage_index);
                let relocated_new_index = garbage_index;

                // mark the removed node's children for deletion
                if let Node::Garbage {
                    children
                } = removed_node.into_inner() {
                    for &child_id in children.as_slice() {
                        if let ChildId {
                            index: Some(child_index)
                        } = child_id {
                            // we've found a node to mark as garbage
                            garbage_vec.push(child_index);
                            if let &Node::Present {
                                ref parent,
                                ..
                            } = &*nodes[child_index].get() {
                                parent.set(ParentId::Garbage);
                            }
                            //(&*nodes[child_index].get()).parent.set(ParentId::Garbage);
                            // TODO

                            //garbage_vec.push(child_index);
                        }
                    }
                } // else, it means we got here because the parent was marked

                let relocated_old_index = nodes.len();
                if relocated_new_index == relocated_old_index {
                    // we don't need to perform reattachment if we removed the last node in the vec
                    // that would actually cause a panic
                    continue;
                }

                let relocated_node = &mut*(&nodes[relocated_new_index]).get();

                match relocated_node {
                    &mut Node::Garbage { .. } => {
                        garbage_vec.push(relocated_new_index);
                    }
                    &mut Node::Present {
                        ref mut parent,
                        ref mut children,
                        ..
                    } => {
                        // reconnect parent
                        match parent.get() {
                            ParentId::Some {
                                parent_index,
                                this_branch,
                            } => {
                                let parent_node = &*(&nodes[parent_index]).get();
                                match parent_node {
                                    &Node::Present {
                                        ref children,
                                        ..
                                    } => {
                                        (&mut *children.get()).as_mut_slice()[this_branch] = ChildId {
                                            index: Some(relocated_new_index),
                                        };
                                    },
                                    &Node::Garbage { .. } => {
                                        unreachable!("node parent is garbage at garbage collection time");
                                    }
                                }
                            },
                            ParentId::Root => {
                                self.root.set(Some(relocated_new_index));
                            },
                            ParentId::Garbage => (),
                            ParentId::Detached => {
                                unreachable!("found detached node on garbage collection sweep");
                            }
                        };

                        // reconnect children
                        for (b, child_id) in (&*children.get()).as_slice().iter().enumerate() {
                            if let &ChildId {
                                index: Some(child_index)
                            } = child_id {

                                let child_node = &*(&nodes[child_index]).get();
                                match child_node {
                                    &Node::Present {
                                        ref parent,
                                        ..
                                    } => {
                                        debug_assert_eq!(parent.get(), ParentId::Some {
                                            parent_index: relocated_old_index,
                                            this_branch: b,
                                        });
                                        parent.set(ParentId::Some {
                                            parent_index: relocated_new_index,
                                            this_branch: b,
                                        });
                                    }
                                    &Node::Garbage { .. } => {
                                        unreachable!("node child is garbage at garbage collection time");
                                    }
                                };
                            }
                        }
                    }
                };
            }
        }
    }
}
unsafe impl<T: Send, C: FixedSizeArray<ChildId>> Send for Tree<T, C> {}
unsafe impl<T: Sync, C: FixedSizeArray<ChildId>> Sync for Tree<T, C> {}
impl<T: Debug, C: FixedSizeArray<ChildId>> Debug for Tree<T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.read_root().fmt(f)
    }
}

pub struct TreeOperation<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree mut Tree<T, C>,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> !Send for TreeOperation<'tree, T, C> {}
impl<'tree, T, C: FixedSizeArray<ChildId>> !Sync for TreeOperation<'tree, T, C> {}
impl<'tree, T, C: FixedSizeArray<ChildId>> Deref for TreeOperation<'tree, T, C> {
    type Target = Tree<T, C>;

    fn deref(&self) -> &<Self as Deref>::Target {
        self.tree
    }
}
impl<'tree, T: Debug, C: FixedSizeArray<ChildId>> Debug for TreeOperation<'tree, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.tree.fmt(f)
    }
}
impl<'tree, T, C: FixedSizeArray<ChildId>> TreeOperation<'tree, T, C> {
    pub fn write_root<'s: 'tree>(&'s self) -> Option<NodeWriteGuard<'s, 's, 'tree, T, C>> {
        let self_immutable: &Self = self;

        self_immutable.root.get()
            .map(|root_index| NodeWriteGuard {
                op: self_immutable,
                index: root_index,

                p1: PhantomData,
            })
    }

    pub fn take_root<'s: 'tree>(&'s self) -> Option<NodeOwnedGuard<'s, 'tree, T, C>> {
        self.root.get()
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
                self.root.set(None);

                // create the guard
                NodeOwnedGuard {
                    op: self,
                    index: root_index,
                    reattached: false,
                }
            })
    }

    unsafe fn delete_root(&self, nodes_vec: &mut PinnedVec<UnsafeCell<Node<T, C>>>) -> bool {
        if let Some(former_root_index) = self.root.get() {
            (&mut*nodes_vec[former_root_index].get()).take_elem_become_garbage();
            (&mut*self.garbage.get()).push(former_root_index);
            true
        } else {
            false
        }
    }

    pub fn put_root_elem(&self, elem: T) -> bool {
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
            self.root.set(Some(child_index));

            // done
            deleted
        }
    }

    pub fn put_root_tree<'s: 'tree>(&self, mut subtree: NodeOwnedGuard<'s, 'tree, T, C>) -> bool {
        unsafe {
            let nodes_vec = &mut*self.nodes.get();

            // mark any existing root as garbage
            let deleted = self.delete_root(nodes_vec);

            // attach the root
            self.root.set(Some(subtree.index));

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

    pub fn traverse_root<'s: 'tree>(&'s mut self) -> Option<TreeWriteTraverser<'tree, 's, T, C>> {
        self.tree.root.get()
            .map(move |root_index| TreeWriteTraverser {
                op: self,
                index: Cell::new(root_index),
            })
    }

    // TODO: could you give it a write guard from another tree?
    // TODO: macro
    /*
    pub fn traverse_from<'s: 'tree>(&'s mut self, guard: NodeReadGuard<'tree, T, C>)
        -> TreeWriteTraverser<'s, T, C> {
        TreeWriteTraverser {
            op: self,
            index: Cell::new(guard.index)
        }
    }
    */
    pub fn traverse_from<'s>(&'s mut self, index: NodeIndex) -> Option<TreeWriteTraverser<'tree, 's, T, C>> {
        if index.index < unsafe { (&*self.nodes.get()).len() } {
            Some(TreeWriteTraverser {
                op: self,
                index: Cell::new(index.index),
            })
        } else {
            None
        }
    }

    pub fn new_detached<'s: 'tree>(&'s self, elem: T) -> NodeOwnedGuard<'s, 'tree, T, C> {
        unsafe {
            // create the new node
            let node = Node::Present {
                elem: UnsafeCell::new(elem),
                parent: Cell::new(ParentId::Detached),
                children: UnsafeCell::new(new_child_array()),
            };

            let node_vec = &mut *self.nodes.get();

            // add it to the vec
            node_vec.push(UnsafeCell::new(node));
            let node_index = node_vec.len() - 1;

            // create the guard
            NodeOwnedGuard {
                op: self,
                index: node_index,
                reattached: false,
            }
        }
    }
}
impl<'tree, T, C: FixedSizeArray<ChildId>> Drop for TreeOperation<'tree, T, C> {
    fn drop(&mut self) {
        self.tree.garbage_collect();
    }
}

pub struct NodeWriteGuard<'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> {
    pub op: &'op TreeOperation<'t, T, C>,
    index: usize,

    p1: PhantomData<&'node mut ()>,
}
impl<'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> NodeWriteGuard<'op, 'node, 't, T, C> {
    pub fn read<'s: 'op>(&'s self) -> NodeReadGuard<'s, T, C> {
        unsafe {
            NodeReadGuard::new(self.op, self.index)
        }
    }

    // TODO: is this lifetime right?
    pub fn into_read(self) -> NodeReadGuard<'node, T, C> where 'op: 'node {
        unsafe {
            NodeReadGuard::new(self.op, self.index)
        }
    }

    unsafe fn unsafe_split<'a>(&mut self) -> (&'a mut T, ChildWriteGuard<'op, 'a, 't, T, C>) {
        if let &Node::Present {
            ref elem,
            ..
        } = &*(&*self.op.nodes.get())[self.index].get() {
            //let elem: &'this mut T = &mut*elem.get();
            let elem = &mut*elem.get();
            //let child_guard: ChildWriteGuard<'tree, 'this, T, C> = ChildWriteGuard {
            let child_guard = ChildWriteGuard {
                op: self.op,
                index: self.index,

                p1: PhantomData,
            };
            (elem, child_guard)
        } else {
            unreachable!("guarding garbage")
        }
    }

    pub fn borrow_split<'a>(&'a mut self) -> (&'a mut T, ChildWriteGuard<'op, 'a, 't, T, C>) {
        unsafe {
            self.unsafe_split()
        }
    }

    pub fn into_split(mut self) -> (&'node mut T, ChildWriteGuard<'op, 'node, 't, T, C>) {
        unsafe {
            self.unsafe_split()
        }
    }

    pub fn elem(&mut self) -> &mut T {
        self.borrow_split().0
    }

    pub fn children<'a>(&'a mut self) -> ChildWriteGuard<'op, 'a, 't, T, C> {
        self.borrow_split().1
    }

    pub fn detach(self) -> NodeOwnedGuard<'op, 't, T, C> {
        unsafe {
            // find and detach the parent
            let parent: ParentId = if let &Node::Present {
                parent: ref parent_cell,
                ..
            } = &*(&*self.op.nodes.get())[self.index].get() {
                let parent = parent_cell.get();
                parent_cell.set(ParentId::Detached);
                parent
            } else {
                unreachable!("write guard index points to garbage")
            };

            // detach the child
            match parent {
                ParentId::Some {
                    parent_index,
                    this_branch
                } => {
                    // detach from a parent node
                    if let &Node::Present {
                        ref children,
                        ..
                    } = &*(&*self.op.nodes.get())[parent_index].get() {
                        (&mut*children.get()).as_mut_slice()[this_branch] = ChildId {
                            index: None
                        };
                    } else {
                        unreachable!("write guard parent index points to garbage");
                    }
                },
                ParentId::Root => {
                    // detach from the root
                    self.op.root.set(None);
                },
                ParentId::Detached => {
                    unreachable!("node owned guard trying to detach node which is already detached");
                }
                ParentId::Garbage => {
                    unreachable!("garbage parent node encountered outside of GC");
                }
            };

            // create the guard
            NodeOwnedGuard {
                op: self.op,
                index: self.index,
                reattached: false
            }
        }
    }
}
impl<'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> !Send for NodeWriteGuard<'op, 'node, 't, T, C> {}
impl<'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> !Sync for NodeWriteGuard<'op, 'node, 't, T, C> {}
impl<'op: 't, 'node, 't, T: Debug, C: FixedSizeArray<ChildId>> Debug for NodeWriteGuard<'op, 'node, 't, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.read().fmt(f)
    }
}

pub struct NodeOwnedGuard<'op: 't, 't, T, C: FixedSizeArray<ChildId>> {
    pub op: &'op TreeOperation<'t, T, C>,
    index: usize,
    reattached: bool,
}
impl<'op: 't, 't, T, C: FixedSizeArray<ChildId>> NodeOwnedGuard<'op, 't, T, C> {
    pub fn read<'s: 'op>(&'s self) -> NodeReadGuard<'s, T, C> {
        unsafe {
            NodeReadGuard::new(self.op, self.index)
        }
    }

    pub fn borrow<'s>(&'s mut self) -> NodeWriteGuard<'op, 't, 's, T, C> {
        NodeWriteGuard {
            op: self.op,
            index: self.index,

            p1: PhantomData,
        }
    }

    pub fn split<'b>(&'b mut self) -> (&'b mut T, ChildWriteGuard<'op, 'b, 't, T, C>) {
        unsafe {
            if let &Node::Present {
                ref elem,
                ..
            } = &*(&*self.op.nodes.get())[self.index].get() {
                let elem = &mut*elem.get();
                let child_guard = ChildWriteGuard {
                    op: self.op,
                    index: self.index,

                    p1: PhantomData,
                };
                (elem, child_guard)
            } else {
                unreachable!("write-guarding garbage")
            }
        }
    }

    pub fn elem(&mut self) -> &mut T {
        self.split().0
    }

    pub fn children<'a>(&'a mut self) -> ChildWriteGuard<'op, 'a, 't, T, C> {
        self.split().1
    }

    pub fn into_elem(mut self) -> T {
        unsafe {
            // acquire a mutable reference to the node
            let node: &mut Node<T, C> = &mut*((&*(&(&*self.op.nodes.get())[self.index])).get());

            // swap it with a garbage node, extract the element
            let elem = node.take_elem_become_garbage();

            // we've marked self as garbage, so we must add self to the garbage vec
            let garbage_vec = &mut*self.op.garbage.get();
            garbage_vec.push(self.index);

            // now we can mark ourself as reattached and drop
            self.reattached = true;
            mem::drop(self);

            // done
            elem
        }
    }
}
impl<'op: 't, 't, T, C: FixedSizeArray<ChildId>> Drop for NodeOwnedGuard<'op, 't, T, C> {
    fn drop(&mut self) {
        if !self.reattached {
            unsafe {
                (&mut*((&(&*(self.op.nodes.get()))[self.index]).get())).take_elem_become_garbage();
                let garbage_vec = &mut*self.op.garbage.get();
                garbage_vec.push(self.index);
            }
        }
    }
}
impl<'op: 't, 't, T: Debug, C: FixedSizeArray<ChildId>> Debug for NodeOwnedGuard<'op, 't, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.read().fmt(f)
    }
}

pub struct ChildWriteGuard<'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> {
    pub op: &'op TreeOperation<'t, T, C>,
    index: usize,

    p1: PhantomData<&'node mut ()>,
}
impl<'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> ChildWriteGuard<'op, 'node, 't, T, C> {
    fn children(&mut self) -> &mut C {
        unsafe {
            if let &Node::Present {
                ref children,
                ..
            } = &*(&(&*self.op.nodes.get())[self.index]).get() {
                &mut *children.get()
            } else {
                unreachable!("child write guard points to garbage node")
            }
        }
    }

    unsafe fn make_child_write_guard<'n>(&mut self, branch: usize)
        -> Result<Option<NodeWriteGuard<'op, 'n, 't, T, C>>, InvalidBranchIndex> {
        self.children().as_slice().get(branch)
            .ok_or(InvalidBranchIndex(branch))
            .map(|child_id| child_id.index)
            .map(|child_index| child_index
                .map(move |child_index| NodeWriteGuard {
                    op: self.op,
                    index: child_index,

                    p1: PhantomData,
                }))
    }

    pub fn borrow_child_write<'s>(&'s mut self, branch: usize)
        -> Result<Option<NodeWriteGuard<'op, 's, 't, T, C>>, InvalidBranchIndex> {
        unsafe {
            self.make_child_write_guard(branch)
        }
    }

    pub fn into_child_write(mut self, branch: usize)
        -> Result<Option<NodeWriteGuard<'op, 'node, 't, T, C>>, InvalidBranchIndex> {
        unsafe {
            self.make_child_write_guard(branch)
        }
    }

    pub fn into_all_children_write(mut self,
                                   mut consumer: impl FnMut(usize, Option<NodeWriteGuard<'op, 'node, 't, T, C>>)) {
        unsafe {
            let branch_factor = {
                let array: C = mem::uninitialized();
                let size = array.as_slice().len();
                mem::forget(array);
                size
            };
            for branch in 0..branch_factor {
                consumer(branch, self.make_child_write_guard(branch).unwrap())
            }
        }
    }

    pub fn take_child(&mut self, branch: usize) -> Result<Option<NodeOwnedGuard<'op, 't, T, C>>, InvalidBranchIndex> {

        self.children().as_slice().get(branch)
            .ok_or(InvalidBranchIndex(branch))
            .map(|child_id| child_id.index)
            .map(|child_index| child_index
                .map(move |child_index| {
                    // detach the parent
                    unsafe {
                        if let &Node::Present {
                            ref parent,
                            ..
                        } = &*(&*self.op.nodes.get())[child_index].get() {
                            parent.set(ParentId::Detached);
                        } else {
                            unreachable!("child index points to garbage");
                        }
                    }

                    // detach the child
                    self.children().as_mut_slice()[branch] = ChildId {
                        index: None
                    };

                    // create the guard
                    NodeOwnedGuard {
                        op: self.op,
                        index: child_index,
                        reattached: false,
                    }
                })
            )
    }

    unsafe fn delete_child(&mut self,
                           nodes_vec: &mut PinnedVec<UnsafeCell<Node<T, C>>>,
                           branch: usize) -> bool {
        if let ChildId {
            index: Some(former_child_index)
        } = self.children().as_slice()[branch] {
            (&mut*nodes_vec[former_child_index].get()).take_elem_become_garbage();
            (&mut*self.op.garbage.get()).push(former_child_index);
            true
        } else {
            false
        }
    }

    pub fn put_child_elem(&mut self, branch: usize, elem: T) -> Result<bool, InvalidBranchIndex> {
        unsafe {
            // short-circuit if the branch is invalid
            if branch >= self.children().as_slice().len() {
                return Err(InvalidBranchIndex(branch));
            }

            // unsafely create the new children array
            let child_children: C = new_child_array();

            // create the new node
            let child_node = Node::Present {
                elem: UnsafeCell::new(elem),
                parent: Cell::new(ParentId::Some {
                    parent_index: self.index,
                    this_branch: branch,
                }),
                children: UnsafeCell::new(child_children)
            };

            let nodes_vec = &mut*self.op.nodes.get();

            // insert it into the nodes vector, get the index
            nodes_vec.push(UnsafeCell::new(child_node));
            let child_index = nodes_vec.len() - 1;

            // mark any existing child as garbage
            let deleted = self.delete_child(nodes_vec, branch);

            // attach the child
            self.children().as_mut_slice()[branch] = ChildId {
                index: Some(child_index)
            };

            // done
            Ok(deleted)
        }
    }

    pub fn put_child_tree(&mut self, branch: usize, mut subtree: NodeOwnedGuard<'op, 't, T, C>) -> Result<bool, InvalidBranchIndex> {
        unsafe {
            // short-circuit if the branch is invalid
            if branch >= self.children().as_slice().len() {
                return Err(InvalidBranchIndex(branch));
            }

            let nodes_vec = &mut*self.op.nodes.get();

            // mark any existing child as garbage
            let deleted = self.delete_child(nodes_vec, branch);

            // attach the child
            self.children().as_mut_slice()[branch] = ChildId {
                index: Some(subtree.index),
            };

            // attach the parent
            if let &Node::Present {
                ref parent,
                ..
            } = &*nodes_vec[subtree.index].get() {
                debug_assert_eq!(parent.get(), ParentId::Detached);
                parent.set(ParentId::Some {
                    parent_index: self.index,
                    this_branch: branch,
                });
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
impl<'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> !Send for ChildWriteGuard<'op, 'node, 't, T, C> {}
impl<'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> !Sync for ChildWriteGuard<'op, 'node, 't, T, C> {}

#[derive(Debug)]
pub struct ChildNotFound(pub usize);

#[derive(Debug)]
pub struct HitTopOfTree;

#[derive(Debug)]
pub struct ThisIsRoot;

pub struct TreeWriteTraverser<'op: 't, 't, T, C: FixedSizeArray<ChildId>> {
    pub op: &'op mut TreeOperation<'t, T, C>,
    index: Cell<usize>,
}
impl<'op: 't, 't, T, C: FixedSizeArray<ChildId>> TreeWriteTraverser<'op, 't, T, C> {
    pub fn into_write_guard(self) -> NodeWriteGuard<'op, 'op, 't, T, C> {
        NodeWriteGuard {
            op: self.op,
            index: self.index.get(),

            p1: PhantomData,
        }
    }

    pub fn into_read_guard(self) -> NodeReadGuard<'op, T, C> {
        unsafe {
            NodeReadGuard::new(self.op.tree, self.index.get())
        }
    }

    pub fn as_write_guard<'s>(&'s mut self) -> NodeWriteGuard<'s, 's, 't, T, C> {
        NodeWriteGuard {
            op: self.op,
            index: self.index.get(),

            p1: PhantomData,
        }
    }

    pub fn is_root(&self) -> bool {
        unsafe {
            if let &mut Node::Present {
                ref parent,
                ..
            } = self.access_node_ref() {
                parent.get() == ParentId::Root
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    pub fn seek_parent(&self) -> Result<(), HitTopOfTree> {
        unsafe {
            if let &mut Node::Present {
                ref parent,
                ..
            } = self.access_node_ref() {
                match parent.get() {
                    ParentId::Some {
                        parent_index,
                        ..
                    } => {
                        self.index.set(parent_index);
                        Ok(())
                    },
                    ParentId::Root => Err(HitTopOfTree),
                    ParentId::Detached => unreachable!("tree write traverser points to detached node"),
                    ParentId::Garbage => unreachable!("garbage parent node encountered outside of GC"),
                }
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    pub fn has_child(&self, branch: usize) -> Result<bool, InvalidBranchIndex> {
        unsafe {
            if let &mut Node::Present {
                ref children,
                ..
            } = self.access_node_ref() {
                (&*children.get()).as_slice()
                    .get(branch)
                    .ok_or(InvalidBranchIndex(branch))
                    .map(|child_id| child_id.index.is_some())
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    pub fn seek_child(&self, branch: usize) -> Result<Result<(), ChildNotFound>, InvalidBranchIndex> {
        unsafe {
            if let &mut Node::Present {
                ref children,
                ..
            } = self.access_node_ref() {
                (&*children.get()).as_slice()
                    .get(branch)
                    .ok_or(InvalidBranchIndex(branch))
                    .map(|child_id| match child_id.index {
                        Some(child_index) => {
                            // acquired child index
                            self.index.set(child_index);
                            Ok(())
                        },
                        None => Err(ChildNotFound(branch))
                    })
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    pub fn detach_this(self) -> NodeOwnedGuard<'op, 't, T, C> {
        unsafe {
            // detach the parent from this
            let old_parent = if let &mut Node::Present {
                ref parent,
                ..
            } = self.access_node_ref() {
                let old_parent = parent.get();
                parent.set(ParentId::Detached);
                old_parent
            } else {
                unreachable!("tree write traverser points to garbage node")
            };

            // detach this from the parent
            match old_parent {
                ParentId::Some {
                    parent_index,
                    this_branch,
                } => {
                    // detach this from another node
                    if let &mut Node::Present {
                        ref children,
                        ..
                    } = &mut*((&mut*self.op.nodes.get())[parent_index].get()) {
                        (&mut*children.get()).as_mut_slice()[this_branch] = ChildId {
                            index: None
                        };
                    } else {
                        unreachable!("tree write traverser parent is garbage");
                    }
                },
                ParentId::Root => {
                    // detach this from the root
                    self.op.tree.root.set(None);
                },
                ParentId::Detached => unreachable!("tree write traverser points to detached"),
                ParentId::Garbage => unreachable!("garbage parent node encountered outside of GC"),
            };

            // create the guard
            NodeOwnedGuard {
                op: self.op,
                index: self.index.get(),
                reattached: false
            }
        }
    }

    pub fn detach_child<'s>(&'s self, branch: usize)
        -> Result<Result<NodeOwnedGuard<'op, 't, T, C>, ChildNotFound>, InvalidBranchIndex> {
        unsafe {
            if let &mut Node::Present {
                ref children,
                ..
            } = self.access_node_ref() {
                let mut children_slice = (&mut*children.get()).as_mut_slice();
                children_slice
                    .get(branch).cloned()
                    .ok_or(InvalidBranchIndex(branch))
                    .map(|child_id| match child_id.index {
                        Some(child_index) => {
                            // acquired child index
                            // detach the child
                            children_slice[branch] = ChildId {
                                index: None
                            };

                            // detach the parent
                            if let &Node::Present {
                                ref parent,
                                ..
                            } = &*(&*self.op.nodes.get())[child_index].get() {
                                parent.set(ParentId::Detached);
                            } else {
                                unreachable!("child index points to garbage");
                            }

                            // create the gaurd
                            Ok(NodeOwnedGuard {
                                op: &*(self.op as *const TreeOperation<'t, T, C>),
                                index: child_index,
                                reattached: false
                            })
                        },
                        None => Err(ChildNotFound(branch))
                    })
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    unsafe fn access_node_ref(&self) -> &mut Node<T, C> {
        &mut*((&mut*self.op.nodes.get())[self.index.get()].get())
    }

    unsafe fn access_elem_ref(&self) -> &mut T {
        if let &mut Node::Present {
            ref elem,
            ..
        } = self.access_node_ref() {
            &mut*elem.get()
        } else {
            unreachable!("tree write traverser points to garbage node")
        }
    }

    pub fn this_branch_index(&self) -> Result<usize, ThisIsRoot> {
        unsafe {
            if let &mut Node::Present {
                ref parent,
                ..
            } = self.access_node_ref() {
                match parent.get() {
                    ParentId::Some {
                        this_branch,
                        ..
                    } => Ok(this_branch),
                    ParentId::Root => Err(ThisIsRoot),
                    ParentId::Detached => unreachable!("tree write traverser points to detached node"),
                    ParentId::Garbage => unreachable!("garbage parent node encountered outside of GC"),
                }
            } else {
                unreachable!("tree write traverser points to garbage")
            }
        }
    }
}
impl<'op: 't, 't, T, C: FixedSizeArray<ChildId>> Deref for TreeWriteTraverser<'op, 't, T, C> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            self.access_elem_ref()
        }
    }
}
impl<'op: 't, 't, T, C: FixedSizeArray<ChildId>> DerefMut for TreeWriteTraverser<'op, 't, T, C> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            self.access_elem_ref()
        }
    }
}

pub struct NodeReadGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    node: &'tree Node<T, C>,
    index: usize,
    pub elem: &'tree T,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> Deref for NodeReadGuard<'tree, T, C> {
    type Target = T;

    fn deref(&self) -> &<Self as Deref>::Target {
        self.elem
    }
}
impl<'tree, T, C: FixedSizeArray<ChildId>> NodeReadGuard<'tree, T, C> {
    unsafe fn new(tree: &'tree Tree<T, C>, index: usize) -> Self {
        let node = &*(&*tree.nodes.get())[index].get();
        let elem = match node {
            &Node::Present {
                ref elem,
                ..
            } => &*elem.get(),
            &Node::Garbage { .. } => unreachable!("new node read guard from garbage"),
        };
        NodeReadGuard {
            tree,
            node,
            index,
            elem,
        }
    }

    pub fn child(&self, branch: usize) -> Result<Option<Self>, InvalidBranchIndex> {
        if let &Node::Present {
            ref children,
            ..
        } = self.node {
            unsafe {
                (&*children.get()).as_slice().get(branch)
                    .ok_or(InvalidBranchIndex(branch))
                    .map(|child_id| child_id.index
                        .map(|child_index| Self::new(self.tree, child_index)))
            }
        } else {
            unreachable!("read guard on garbage node")
        }
    }

    pub fn index(&self) -> NodeIndex {
        NodeIndex {
            index: self.index
        }
    }
}
impl<'tree, T: Debug, C: FixedSizeArray<ChildId>> Debug for NodeReadGuard<'tree, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let mut builder = f.debug_struct("Node");
        builder.field("elem", self.elem);
        let num_children = match self.node {
            &Node::Present {
                ref children,
                ..
            } => unsafe { (&*children.get()).as_slice().len() },
            &Node::Garbage { .. } => unreachable!("node read guard on garbage"),
        };
        for branch in 0..num_children {
            builder.field(&format!("child_{}", branch), &self.child(branch).unwrap());
        }
        builder.finish()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct NodeIndex {
    index: usize,
}


// TODO: iteration
// TODO: docs and example
// TODO: automated test
