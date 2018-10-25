#![feature(fixed_size_array)]
#![feature(optin_builtin_traits)]

extern crate core;

pub mod bst;
mod pinned_vec;

use pinned_vec::PinnedVec;

use core::array::FixedSizeArray;
use std::cell::{UnsafeCell, Cell};
use std::ops::{Deref, Drop};
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

    pub fn garbage_collect(&mut self) {
        unsafe {
            let garbage_vec = &mut*self.garbage.get();
            let nodes = &mut*self.nodes.get();

            nodes.compress();

            while let Some(garbage_index) = garbage_vec.pop() {
                if garbage_index >= nodes.len() {
                    continue;
                }

                debug_assert!(match &*(&(&*self.nodes.get())[garbage_index]).get() {
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
                            garbage_vec.push(child_index);
                        }
                    }
                } // else, it means we got here because the parent was marked

                if garbage_index == relocated_new_index {
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
                        ..
                    } => match parent.get() {
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
                                    (&mut*children.get()).as_mut_slice()[this_branch] = ChildId {
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
                        ParentId::Detached => {
                            unreachable!("found detached node on garbage collection sweep");
                        }
                    },
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
    pub fn write_root<'this: 'tree>(&'this self) -> Option<NodeWriteGuard<'tree, 'this, T, C>> {
        let self_immutable: &Self = self;

        self_immutable.root.get()
            .map(|root_index| NodeWriteGuard {
                op: self_immutable,
                index: root_index,

                p1: PhantomData,
            })
    }

    pub fn take_root<'this: 'tree>(&'this self) -> Option<NodeOwnedGuard<'tree, T, C>> {
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

    pub fn put_root_tree(&self, mut subtree: NodeOwnedGuard<'tree, T, C>) -> bool {
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

    pub fn new_detached<'s: 'tree>(&'s self, elem: T) -> NodeOwnedGuard<'tree, T, C> {
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

    pub fn finish_and_gc(self) {
        self.tree.garbage_collect();
    }
}

pub struct NodeWriteGuard<'tree, 'node, T, C: FixedSizeArray<ChildId>> {
    pub op: &'tree TreeOperation<'tree, T, C>,
    index: usize,

    p1: PhantomData<&'node mut ()>,
}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> NodeWriteGuard<'tree, 'node, T, C> {
    pub fn read<'s: 'tree>(&'s self) -> NodeReadGuard<'s, T, C> {
        unsafe {
            NodeReadGuard::new(self.op, self.index)
        }
    }

    unsafe fn unsafe_split<'a>(&mut self) -> (&'a mut T, ChildWriteGuard<'tree, 'a, T, C>) {
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

    pub fn borrow_split<'a>(&'a mut self) -> (&'a mut T, ChildWriteGuard<'tree, 'a, T, C>) {
        unsafe {
            self.unsafe_split()
        }
    }

    pub fn into_split(mut self) -> (&'node mut T, ChildWriteGuard<'tree, 'node, T, C>) {
        unsafe {
            self.unsafe_split()
        }
    }

    pub fn elem(&mut self) -> &mut T {
        self.borrow_split().0
    }

    pub fn children<'a>(&'a mut self) -> ChildWriteGuard<'tree, 'a, T, C> {
        self.borrow_split().1
    }

    pub fn detach(self) -> NodeOwnedGuard<'tree, T, C> {
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
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> !Send for NodeWriteGuard<'tree, 'node, T, C> {}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> !Sync for NodeWriteGuard<'tree, 'node, T, C> {}
impl<'tree, 'node, T: Debug, C: FixedSizeArray<ChildId>> Debug for NodeWriteGuard<'tree, 'node, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.read().fmt(f)
    }
}

pub struct NodeOwnedGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    pub op: &'tree TreeOperation<'tree, T, C>,
    index: usize,
    reattached: bool,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> NodeOwnedGuard<'tree, T, C> {
    pub fn read<'s: 'tree>(&'s self) -> NodeReadGuard<'s, T, C> {
        unsafe {
            NodeReadGuard::new(self.op, self.index)
        }
    }

    pub fn borrow<'s>(&'s mut self) -> NodeWriteGuard<'tree, 's, T, C> {
        NodeWriteGuard {
            op: self.op,
            index: self.index,

            p1: PhantomData,
        }
    }

    pub fn split<'b>(&'b mut self) -> (&'b mut T, ChildWriteGuard<'tree, 'b, T, C>) {
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

    pub fn children<'a>(&'a mut self) -> ChildWriteGuard<'tree, 'a, T, C> {
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
impl<'tree, T, C: FixedSizeArray<ChildId>> Drop for NodeOwnedGuard<'tree, T, C> {
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
impl<'tree, T: Debug, C: FixedSizeArray<ChildId>> Debug for NodeOwnedGuard<'tree, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.read().fmt(f)
    }
}

pub struct ChildWriteGuard<'tree, 'node, T, C: FixedSizeArray<ChildId>> {
    pub op: &'tree TreeOperation<'tree, T, C>,
    index: usize,

    p1: PhantomData<&'node mut ()>,
}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> ChildWriteGuard<'tree, 'node, T, C> {
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
        -> Result<Option<NodeWriteGuard<'tree, 'n, T, C>>, InvalidBranchIndex> {
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
        -> Result<Option<NodeWriteGuard<'tree, 's, T, C>>, InvalidBranchIndex> {
        unsafe {
            self.make_child_write_guard(branch)
        }
    }

    pub fn into_child_write(mut self, branch: usize)
        -> Result<Option<NodeWriteGuard<'tree, 'node, T, C>>, InvalidBranchIndex> {
        unsafe {
            self.make_child_write_guard(branch)
        }
    }

    pub fn into_all_children_write(mut self,
                                   mut consumer: impl FnMut(usize, Option<NodeWriteGuard<'tree, 'node, T, C>>)) {
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

    pub fn take_child(&mut self, branch: usize) -> Result<Option<NodeOwnedGuard<'tree, T, C>>, InvalidBranchIndex> {

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

    pub fn put_child_tree(&mut self, branch: usize, mut subtree: NodeOwnedGuard<'tree, T, C>) -> Result<bool, InvalidBranchIndex> {
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
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> !Send for ChildWriteGuard<'tree, 'node, T, C> {}
impl<'tree, 'node, T, C: FixedSizeArray<ChildId>> !Sync for ChildWriteGuard<'tree, 'node, T, C> {}

pub struct NodeReadGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    node: &'tree Node<T, C>,
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
            elem
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

// TODO: iteration
// TODO: docs and example
// TODO: automated test
