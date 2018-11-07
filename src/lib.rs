#![feature(fixed_size_array)]
#![feature(optin_builtin_traits)]

extern crate core;

mod pinned_vec;
#[cfg(test)]
mod test;

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

/// Types that can be converted into a NodeReadGuard.
pub trait IntoReadGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    fn into_read_guard(self) -> NodeReadGuard<'tree, T, C>;
}

/// Types that can be convered into a NodeWriteGuard.
pub trait IntoWriteGuard<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> {
    fn into_write_guard(self) -> NodeWriteGuard<'op, 'node, 't, T, C>;
}

/// An opaque type used for genericity over branch factor.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct ChildId {
    index: Option<usize>
}
impl Debug for ChildId {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        f.write_str(&format!("{:?}", self.index))
    }
}

/// Error type for performing operations on a branch index that does not exist in the
/// given branch factor.
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

/// A struct which borrows from the tree, and allows the debug printing of the tree's
/// node vector, for debugging purposes.
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

/// The top-level tree type, generic over element type and branch factor.
///
/// The `Tree`, and nearly all components borrowed from the tree, is generic over two types:
/// the element type, and a sized array of `ChildId`. The element type, `T`, is stored for
/// every node in the tree. Each node may have any combination of children, with any child
/// index that is a valid index of the `ChildId` array.
///
/// For example, a binary search tree set of `i32` could be represented as a `Tree<i32, [ChildId; 2]>`.
pub struct Tree<T, C: FixedSizeArray<ChildId>> {
    nodes: UnsafeCell<PinnedVec<UnsafeCell<Node<T, C>>>>,
    root: Cell<Option<usize>>,
    garbage: UnsafeCell<Vec<usize>>,
}
impl<T, C: FixedSizeArray<ChildId>> Tree<T, C> {
    /// Create a new, empty tree.
    pub fn new() -> Self {
        Tree {
            nodes: UnsafeCell::new(PinnedVec::new(EXTENSION_SIZE)),
            root: Cell::new(None),
            garbage: UnsafeCell::new(Vec::new()),
        }
    }

    /// Get a view of the tree than can be debug printed to see the node vec.
    pub fn debug_nodes(&self) -> DebugNodes<T, C> {
        DebugNodes {
            nodes: &self.nodes
        }
    }

    /// Read the root of the tree, if it exists.
    pub fn read_root<'tree>(&'tree self) -> Option<NodeReadGuard<'tree, T, C>> {
        self.root.get()
            .map(|root_index| unsafe {
                NodeReadGuard::new(self, root_index)
            })
    }

    /// Read-traverse the tree, starting at the root, if it exists.
    pub fn traverse_read_root<'tree>(&'tree self) -> Option<TreeReadTraverser<'tree, T, C>> {
        self.root.get()
            .map(|root_index| unsafe {
                TreeReadTraverser::new(self, root_index)
            })
    }

    /// Begin an operation which can mutate the tree.
    pub fn operation<'tree>(&'tree mut self) -> TreeOperation<'tree, T, C> {
        TreeOperation {
            tree: self
        }
    }

    /// Reallocate the node vec, so that the capacity is no larger than its length.
    pub fn shrink_to_fit(&mut self) {
        unsafe {
            (&mut *self.nodes.get()).shrink_to_fit();
        }
    }

    /// Garbage collect the node vec. This is an O(N) operation, where N is the number of garbage
    /// nodes, and it is automatically done whenever a TreeOperation is dropped. Garbage nodes
    /// will be swap-removed from the node vec, and nodes' child and parent indices will be
    /// updated to maintain the validity of the tree.
    ///
    /// This will cause all non-dropped garbage nodes to be dropped.
    pub fn garbage_collect(&mut self) {
        unsafe {
            let garbage_vec = &mut*self.garbage.get();
            let nodes = &mut*self.nodes.get();

            nodes.defragment();

            while let Some(garbage_index) = garbage_vec.pop() {
                if garbage_index >= nodes.len() {
                    continue;
                }

                debug_assert!(match &*(&nodes[garbage_index]).get() {
                    &Node::Garbage { .. } => true,
                    &Node::Present { .. } => false
                });

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

/// A `TreeOperation` is a type which borrows mutably from the `Tree`, and allows for modification to that
/// tree. While a `TreeOperation` exists, the access to the tree can only be single-threaded. This allows
/// many operations to mutate the tree with only a immutable reference to the `TreeOperation`, directly or
/// indirectly.
pub struct TreeOperation<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree mut Tree<T, C>,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> TreeOperation<'tree, T, C> {
    /// Write to the root of the tree, if it exists.
    pub fn write_root<'s>(&'s mut self) -> Option<NodeWriteGuard<'s, 's, 'tree, T, C>> {
        let self_immutable: &Self = self;

        self_immutable.tree.root.get()
            .map(|root_index| NodeWriteGuard {
                op: self_immutable,
                index: root_index,

                p1: PhantomData,
            })
    }

    /// Detach the root of the tree, if it exists.
    pub fn take_root<'s>(&'s mut self) -> Option<NodeOwnedGuard<'s, 'tree, T, C>> {
        self.tree.root.get()
            .map(move |root_index| {
                // detach the parent
                unsafe {
                    if let &Node::Present {
                        ref parent,
                        ..
                    } = &*(&*self.tree.nodes.get())[root_index].get() {
                        debug_assert_eq!(parent.get(), ParentId::Root);
                        parent.set(ParentId::Detached);
                    } else {
                        unreachable!("root index points to garbage");
                    }
                }

                // detach the root
                self.tree.root.set(None);

                // create the guard
                NodeOwnedGuard {
                    op: self,
                    index: root_index,
                    reattached: false,
                }
            })
    }

    unsafe fn delete_root(&mut self, nodes_vec: &mut PinnedVec<UnsafeCell<Node<T, C>>>) -> bool {
        if let Some(former_root_index) = self.tree.root.get() {
            (&mut*nodes_vec[former_root_index].get()).take_elem_become_garbage();
            (&mut*self.tree.garbage.get()).push(former_root_index);
            true
        } else {
            false
        }
    }

    /// Put an element as the root of the tree, returning whether some existing root was
    /// overridden.
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

            let nodes_vec = &mut*self.tree.nodes.get();

            // insert it into the nodes vector, get the index
            nodes_vec.push(UnsafeCell::new(child_node));
            let child_index = nodes_vec.len() - 1;

            // mark any existing root as garbage
            let deleted = self.delete_root(nodes_vec);

            // attach the root
            self.tree.root.set(Some(child_index));

            // done
            deleted
        }
    }

    /// Put a detached subtree as the root of the tree, returning whether some existing
    /// root was overridden.
    pub fn put_root_tree<'s>(&mut self, mut subtree: NodeOwnedGuard<'s, 'tree, T, C>) -> bool {
        unsafe {
            let nodes_vec = &mut*self.tree.nodes.get();

            // mark any existing root as garbage
            let deleted = self.delete_root(nodes_vec);

            // attach the root
            self.tree.root.set(Some(subtree.index));

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

    /// Begin write-traversing from the root of the tree, if the root exists.
    pub fn traverse_root<'s>(&'s mut self) -> Option<TreeWriteTraverser<'tree, 's, T, C>> {
        self.tree.root.get()
            .map(move |root_index| TreeWriteTraverser {
                op: self,
                index: Cell::new(root_index),
            })
    }

    /// Begin write-traversing from some arbitrary node in the tree.
    ///
    /// Instead of using this method, use the traverse_from! macro.
    pub fn traverse_from<'s>(&'s mut self, index: NodeIndex) -> Option<TreeWriteTraverser<'tree, 's, T, C>> {
        if index.index < unsafe { (&*self.tree.nodes.get()).len() } {
            Some(TreeWriteTraverser {
                op: self,
                index: Cell::new(index.index),
            })
        } else {
            None
        }
    }

    /// Create a new detached subtree.
    pub fn new_detached<'s>(&'s self, elem: T) -> NodeOwnedGuard<'s, 'tree, T, C> {
        unsafe {
            // create the new node
            let node = Node::Present {
                elem: UnsafeCell::new(elem),
                parent: Cell::new(ParentId::Detached),
                children: UnsafeCell::new(new_child_array()),
            };

            let node_vec = &mut *self.tree.nodes.get();

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

    /// Get a view of the tree than can be debug printed to see the node vec.
    pub fn debug_nodes(&self) -> DebugNodes<T, C> {
        self.tree.debug_nodes()
    }

    /// Read the root of the tree, if it exists.
    pub fn read_root<'s>(&'s self) -> Option<NodeReadGuard<'s, T, C>> {
        self.tree.read_root()
    }
}
impl<'tree, T, C: FixedSizeArray<ChildId>> !Send for TreeOperation<'tree, T, C> {}
impl<'tree, T, C: FixedSizeArray<ChildId>> !Sync for TreeOperation<'tree, T, C> {}
impl<'tree, T: Debug, C: FixedSizeArray<ChildId>> Debug for TreeOperation<'tree, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.tree.fmt(f)
    }
}
impl<'tree, T, C: FixedSizeArray<ChildId>> Drop for TreeOperation<'tree, T, C> {
    fn drop(&mut self) {
        self.tree.garbage_collect();
    }
}

/// A `NodeWriteGuard` is a type which holds mutable access to a subset of the tree (a node and all its
/// children, recursively). The `NodeWriteGuard` can be simultaneously borrowed into a mutable reference
/// to the element (`&mut T`) and a `ChildWriteGuard`. The `ChildWriteGuard` has the ability to alter the
/// node's children. Additionally, a `NodeWriteGuard` can be turned into a `NodeOwnedGuard`, detaching the
/// guarded subtree.
///
/// A `NodeWriteGuard` cannot outlive the parent node guard (if the node is root, it cannot
/// outlive the tree).
pub struct NodeWriteGuard<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> {
    pub op: &'op TreeOperation<'t, T, C>,
    index: usize,

    p1: PhantomData<&'node mut ()>,
}
impl<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> NodeWriteGuard<'op, 'node, 't, T, C> {
    unsafe fn unsafe_split<'a>(&mut self) -> (&'a mut T, ChildWriteGuard<'op, 'a, 't, T, C>) {
        if let &Node::Present {
            ref elem,
            ..
        } = &*(&*self.op.tree.nodes.get())[self.index].get() {
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

    /// Split this write guard into mutable access to the element and the children, borrowing from self.
    pub fn borrow_split<'a>(&'a mut self) -> (&'a mut T, ChildWriteGuard<'op, 'a, 't, T, C>) {
        unsafe {
            self.unsafe_split()
        }
    }

    /// Split this write guard into mutable access to the elemtn and the children, consuming self.
    pub fn into_split(mut self) -> (&'node mut T, ChildWriteGuard<'op, 'node, 't, T, C>) {
        unsafe {
            self.unsafe_split()
        }
    }

    /// Mutably access the element.
    pub fn elem(&mut self) -> &mut T {
        self.borrow_split().0
    }

    /// Mutable access the children.
    pub fn children<'a>(&'a mut self) -> ChildWriteGuard<'op, 'a, 't, T, C> {
        self.borrow_split().1
    }

    /// Detach this node from the parent, consuming self, and produced a detached subtree.
    pub fn detach(self) -> NodeOwnedGuard<'op, 't, T, C> {
        unsafe {
            // find and detach the parent
            let parent: ParentId = if let &Node::Present {
                parent: ref parent_cell,
                ..
            } = &*(&*self.op.tree.nodes.get())[self.index].get() {
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
                    } = &*(&*self.op.tree.nodes.get())[parent_index].get() {
                        (&mut*children.get()).as_mut_slice()[this_branch] = ChildId {
                            index: None
                        };
                    } else {
                        unreachable!("write guard parent index points to garbage");
                    }
                },
                ParentId::Root => {
                    // detach from the root
                    self.op.tree.root.set(None);
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
impl<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> !Send for NodeWriteGuard<'op, 'node, 't, T, C> {}
impl<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> !Sync for NodeWriteGuard<'op, 'node, 't, T, C> {}
impl<'op, 'node, 't: 'op, T: Debug, C: FixedSizeArray<ChildId>> Debug for NodeWriteGuard<'op, 'node, 't, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.into_read_guard().fmt(f)
    }
}
impl <'s: 'op, 'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> IntoReadGuard<'s, T, C>
for &'s NodeWriteGuard<'op, 'node, 't, T, C> {
    fn into_read_guard(self) -> NodeReadGuard<'s, T, C> {
        unsafe {
            NodeReadGuard::new(self.op.tree, self.index)
        }
    }
}
impl<'op: 't + 'node, 'node, 't, T, C: FixedSizeArray<ChildId>> IntoReadGuard<'node, T, C>
for NodeWriteGuard<'op, 'node, 't, T, C> {
    fn into_read_guard(self) -> NodeReadGuard<'node, T, C> {
        unsafe {
            NodeReadGuard::new(self.op.tree, self.index)
        }
    }
}
impl<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> IntoWriteGuard<'op, 'node, 't, T, C>
for NodeWriteGuard<'op, 'node, 't, T, C> {
    fn into_write_guard(self) -> NodeWriteGuard<'op, 'node, 't, T, C> {
        self
    }
}
impl<'s: 'node, 'op: 't, 'node, 't, T, C: FixedSizeArray<ChildId>> IntoWriteGuard<'op, 's, 't, T, C>
for &'s mut NodeWriteGuard<'op, 'node, 't, T, C> {
    fn into_write_guard(self) -> NodeWriteGuard<'op, 's, 't, T, C> {
        unimplemented!()
    }
}

/// A owning handle to a detached subtree.
///
/// Normally, a node in a tree is owned by its parent. The root node has a special status, where its parent
/// is considered to be root. However, during the lifetime of a `TreeOperation`, a subtree can be detached
/// from its parent, and it can be considered to be owned by a `NodeOwnedGuard`. In this case, the
/// `NodeOwnedGuard` can outlive the guard for its parent. This subtree can be reattached to the main tree
/// as the child of another node. During the lifetime of the `NodeOwnedGuard`, it is possible to mutate it,
/// as if it were part of the main tree. Alternatively, a `NodeOwnedGuard` can be transformed into its
/// inner element, marking the node and all its children as garbage.
///
/// When a `NodeOwnedGuard` is dropped, and its subtree is marked as garbage, the elements' destructors will
/// run sometime between the dropping of the `NodeOwnedGuard` and the dropping of the `TreeOperation`.
pub struct NodeOwnedGuard<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> {
    pub op: &'op TreeOperation<'t, T, C>,
    index: usize,
    reattached: bool,
}
impl<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> NodeOwnedGuard<'op, 't, T, C> {
    /// Split this owned guard into mutable access to the element and children of the root of this
    /// detached subtree.
    pub fn split<'b>(&'b mut self) -> (&'b mut T, ChildWriteGuard<'op, 'b, 't, T, C>) {
        unsafe {
            if let &Node::Present {
                ref elem,
                ..
            } = &*(&*self.op.tree.nodes.get())[self.index].get() {
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

    /// Mutably access the root element of this detached subtree.
    pub fn elem(&mut self) -> &mut T {
        self.split().0
    }

    /// Mutably access the children of the root of this detached subtree.
    pub fn children<'a>(&'a mut self) -> ChildWriteGuard<'op, 'a, 't, T, C> {
        self.split().1
    }

    /// Consume self, turning this node into garbage, and returning ownership of the element.
    pub fn into_elem(mut self) -> T {
        unsafe {
            // acquire a mutable reference to the node
            let node: &mut Node<T, C> = &mut*((&*(&(&*self.op.tree.nodes.get())[self.index])).get());

            // swap it with a garbage node, extract the element
            let elem = node.take_elem_become_garbage();

            // we've marked self as garbage, so we must add self to the garbage vec
            let garbage_vec = &mut*self.op.tree.garbage.get();
            garbage_vec.push(self.index);

            // now we can mark ourself as reattached and drop
            self.reattached = true;
            mem::drop(self);

            // done
            elem
        }
    }
}
impl<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> Drop for NodeOwnedGuard<'op, 't, T, C> {
    fn drop(&mut self) {
        if !self.reattached {
            unsafe {
                (&mut*((&(&*(self.op.tree.nodes.get()))[self.index]).get())).take_elem_become_garbage();
                let garbage_vec = &mut*self.op.tree.garbage.get();
                garbage_vec.push(self.index);
            }
        }
    }
}
impl<'op, 't: 'op, T: Debug, C: FixedSizeArray<ChildId>> Debug for NodeOwnedGuard<'op, 't, T, C> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.into_read_guard().fmt(f)
    }
}
impl<'s, 'op: 's, 't: 'op, T, C: FixedSizeArray<ChildId>> IntoReadGuard<'s, T, C>
for &'s NodeOwnedGuard<'op, 't, T, C> {
    fn into_read_guard(self) -> NodeReadGuard<'s, T, C> {
        unsafe {
            NodeReadGuard::new(self.op.tree, self.index)
        }
    }
}
impl<'s: 'op, 'op, 't: 'op, T, C: FixedSizeArray<ChildId>> IntoWriteGuard<'op, 't, 's, T, C>
for &'s mut NodeOwnedGuard<'op, 't, T, C> {
    fn into_write_guard(self) -> NodeWriteGuard<'op, 't, 's, T, C> {
        NodeWriteGuard {
            op: self.op,
            index: self.index,

            p1: PhantomData,
        }
    }
}

/// An error type for streaming all children, when the output array is of the wrong size.
#[derive(Debug)]
pub struct WrongChildrenNum {
    pub expected_num: usize,
    pub actual_num: usize,
}

/// Mutable access to a node's children.
///
/// A `ChildWriteGuard` represents mutable access to a node's children. It can be borrowed from both a
/// `NodeWriteGuard` and a `NodeOwnedGuard`. The `ChildWriteGuard` can borrow out a `NodeWriteGuard` to
/// its child, or even several different children simultaneously. Additionally, a `ChildWriteGuard` can
/// put an element as a particular child (marking any previous child subtree as garbage), or even attach
/// an entire detached subtree (a `NodeOwnedGuard`) as one of its children.
pub struct ChildWriteGuard<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> {
    pub op: &'op TreeOperation<'t, T, C>,
    index: usize,

    p1: PhantomData<&'node mut ()>,
}
impl<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> ChildWriteGuard<'op, 'node, 't, T, C> {
    fn children(&mut self) -> &mut C {
        unsafe {
            if let &Node::Present {
                ref children,
                ..
            } = &*(&(&*self.op.tree.nodes.get())[self.index]).get() {
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

    /// Borrow a write guard for a certain child, if that child exists.
    pub fn borrow_child_write<'s>(&'s mut self, branch: usize)
        -> Result<Option<NodeWriteGuard<'op, 's, 't, T, C>>, InvalidBranchIndex> {
        unsafe {
            self.make_child_write_guard(branch)
        }
    }

    /// Turn into the write guard for a certain child, if that child exists.
    pub fn into_child_write(mut self, branch: usize)
        -> Result<Option<NodeWriteGuard<'op, 'node, 't, T, C>>, InvalidBranchIndex> {
        unsafe {
            self.make_child_write_guard(branch)
        }
    }

    /// Stream all child write guards into an array, borrowing from self.
    pub fn borrow_all_children<'s>(&'s mut self,
                                   out: &mut [Option<NodeWriteGuard<'op, 's, 't, T, C>>])
        -> Result<(), WrongChildrenNum> {
        unsafe {
            let branch_factor = {
                let array: C = mem::uninitialized();
                let size = array.as_slice().len();
                mem::forget(array);
                size
            };
            if branch_factor == out.len() {
                for b in 0..branch_factor {
                    out[b] = self.make_child_write_guard(b).unwrap();
                }
                Ok(())
            } else {
                Err(WrongChildrenNum {
                    expected_num: branch_factor,
                    actual_num: out.len()
                })
            }
        }
    }

    /// Stream all child write guards into an array, consuming from self.
    pub fn into_all_children(mut self, out: &mut [Option<NodeWriteGuard<'op, 'node, 't, T, C>>])
        -> Result<(), WrongChildrenNum> {
        unsafe {
            let branch_factor = {
                let array: C = mem::uninitialized();
                let size = array.as_slice().len();
                mem::forget(array);
                size
            };
            if branch_factor == out.len() {
                for b in 0..branch_factor {
                    out[b] = self.make_child_write_guard(b).unwrap();
                }
                Ok(())
            } else {
                Err(WrongChildrenNum {
                    expected_num: branch_factor,
                    actual_num: out.len()
                })
            }
        }
    }

    /// Detach a child, turning it into a detached subtree, if that child exists.
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
                        } = &*(&*self.op.tree.nodes.get())[child_index].get() {
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
            (&mut*self.op.tree.garbage.get()).push(former_child_index);
            true
        } else {
            false
        }
    }

    /// Put an element as a certain child, returning whether any existing child was overridden.
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

            let nodes_vec = &mut*self.op.tree.nodes.get();

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

    /// Attach a detached subtree as a certain child, returning whether any existing child was overridden.
    pub fn put_child_tree(&mut self, branch: usize, mut subtree: NodeOwnedGuard<'op, 't, T, C>)
        -> Result<bool, InvalidBranchIndex> {
        unsafe {
            // short-circuit if the branch is invalid
            if branch >= self.children().as_slice().len() {
                return Err(InvalidBranchIndex(branch));
            }

            let nodes_vec = &mut*self.op.tree.nodes.get();

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
impl<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> !Send for ChildWriteGuard<'op, 'node, 't, T, C> {}
impl<'op, 'node, 't: 'op, T, C: FixedSizeArray<ChildId>> !Sync for ChildWriteGuard<'op, 'node, 't, T, C> {}

/// Error type for attempting to detach a child that doesn't exist.
#[derive(Debug)]
pub struct ChildNotFound(pub usize);

/// Error type for trying to traverse upwards when enable.
#[derive(Debug)]
pub enum NoParent {
    Root,
    Detached
}

/// Possibilities for what owns a certain node.
#[derive(Debug, Eq, PartialEq)]
pub enum AboveMe {
    /// Another node is above this node.
    Parent,
    /// This node is the root of the main tree.
    Root,
    /// This node is the root of a detached subtree.
    Detached,
}

/// While a `NodeWriteGuard` holds mutable access to a subtree (a node and its children, recursively), a
/// `TreeWriteTraverser` holds mutable access to an entire tree. This allows a `TreeWriteTraverser` to
/// seek the parent node, which a `NodeWriteGuard` cannot do. However, this also means that a
/// `TreeWriteTraverser` cannot exist at the same time as any other guards.
///
/// Beyond that difference, a `TreeWriteTraverser` can be used similarly to a `NodeWriteGuard`.
/// A `TreeWriteTraverser` can even be turned or mutably borrowed into a `NodeWriteGuard`.
///
/// A `TreeOperation` and some type of node guard can be conveniently turned into a `TreeWriteTraverser`
/// with the `traverse_from!` macro.
pub struct TreeWriteTraverser<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> {
    pub op: &'op mut TreeOperation<'t, T, C>,
    index: Cell<usize>,
}
impl<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> TreeWriteTraverser<'op, 't, T, C> {
    /// What is above the current node.
    pub fn above_me(&self) -> AboveMe {
        unsafe {
            if let &mut Node::Present {
                ref parent,
                ..
            } = self.access_node_ref() {
                match parent.get() {
                    ParentId::Some { .. } => AboveMe::Parent,
                    ParentId::Root => AboveMe::Root,
                    ParentId::Detached => AboveMe::Detached,
                    ParentId::Garbage => unreachable!("encountered garbage parent outside of GC"),
                }
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    /// Attempt to point this traverser to the parent.
    pub fn seek_parent(&self) -> Result<(), NoParent> {
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
                    ParentId::Root => Err(NoParent::Root),
                    ParentId::Detached => Err(NoParent::Detached),
                    ParentId::Garbage => unreachable!("garbage parent node encountered outside of GC"),
                }
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    /// Does the given child exist.
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

    /// Attempt to point this traverser to the given child.
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

    /// Detach the pointed-at node, consuming this traverser, and producing a detached subtree.
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
                    } = &mut*((&mut*self.op.tree.nodes.get())[parent_index].get()) {
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

    /// Attempt to detach a child node, producing a detached subtree.
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
                            } = &*(&*self.op.tree.nodes.get())[child_index].get() {
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
        &mut*((&mut*self.op.tree.nodes.get())[self.index.get()].get())
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

    /// If the pointed-at node has a parent, what is the branch index of this node.
    pub fn this_branch_index(&self) -> Result<usize, NoParent> {
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
                    ParentId::Root => Err(NoParent::Root),
                    ParentId::Detached => Err(NoParent::Detached),
                    ParentId::Garbage => unreachable!("garbage parent node encountered outside of GC"),
                }
            } else {
                unreachable!("tree write traverser points to garbage")
            }
        }
    }
}
impl<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> Deref for TreeWriteTraverser<'op, 't, T, C> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            self.access_elem_ref()
        }
    }
}
impl<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> DerefMut for TreeWriteTraverser<'op, 't, T, C> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            self.access_elem_ref()
        }
    }
}
impl<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> IntoReadGuard<'op, T, C>
for TreeWriteTraverser<'op, 't, T, C> {
    fn into_read_guard(self) -> NodeReadGuard<'op, T, C> {
        unsafe {
            NodeReadGuard::new(self.op.tree, self.index.get())
        }
    }
}
impl<'s: 'op, 'op, 't: 'op, T, C: FixedSizeArray<ChildId>> IntoReadGuard<'s, T, C>
for &'s TreeWriteTraverser<'op, 't, T, C> {
    fn into_read_guard(self) -> NodeReadGuard<'s, T, C> {
        unsafe {
            NodeReadGuard::new(self.op.tree, self.index.get())
        }
    }
}
impl<'op, 't: 'op, T, C: FixedSizeArray<ChildId>> IntoWriteGuard<'op, 'op, 't, T, C>
for TreeWriteTraverser<'op, 't, T, C> {
    fn into_write_guard(self) -> NodeWriteGuard<'op, 'op, 't, T, C> {
        NodeWriteGuard {
            op: self.op,
            index: self.index.get(),

            p1: PhantomData,
        }
    }
}
impl<'s, 'op: 's, 't: 'op, T, C: FixedSizeArray<ChildId>> IntoWriteGuard<'s, 's, 't, T, C>
for &'s mut TreeWriteTraverser<'op, 't, T, C> {
    fn into_write_guard(self) -> NodeWriteGuard<'s, 's, 't, T, C> {
        NodeWriteGuard {
            op: self.op,
            index: self.index.get(),

            p1: PhantomData,
        }
    }
}

/// A `NodeReadGuard` is a type which holds immutable access to a subset of the tree. Because a
/// `NodeReadGuard` doesn't mutably borrow anything, it does not need to split into a `ChildWriteGuard`
/// and `&mut T`. Instead, the `NodeReadGuard` immutably dereferences to a `T`, and its children
/// can be accessed directly with a method.
#[derive(Copy, Clone)]
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
impl<'tree, T, C: FixedSizeArray<ChildId>> IntoReadGuard<'tree, T, C> for NodeReadGuard<'tree, T, C> {
    fn into_read_guard(self) -> NodeReadGuard<'tree, T, C> {
        self
    }
}

/// The `TreeReadTraverser` is to the `NodeReadGuard` as the `TreeWriteTraverser` is to the `NodeWriteGuard`.
/// While the `NodeReadGuard` holds immutable access to a subset of the tree, a `TreeReadTraverser` holds
/// immutable access to the entire tree. This allows the `TreeReadTraverser` to safely traverse to
/// its parent node.
pub struct TreeReadTraverser<'tree, T, C: FixedSizeArray<ChildId>> {
    pub tree: &'tree Tree<T, C>,
    pub elem: &'tree T,
    index: usize,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> TreeReadTraverser<'tree, T, C> {
    unsafe fn new(tree: &'tree Tree<T, C>, index: usize) -> Self {
        let node = &*(&*tree.nodes.get())[index].get();
        let elem = match node {
            &Node::Present {
                ref elem,
                ..
            } => &*elem.get(),
            &Node::Garbage { .. } => unreachable!("new node read guard from garbage"),
        };
        TreeReadTraverser {
            tree,
            elem,
            index,
        }
    }

    pub fn above_me(&self) -> AboveMe {
        unsafe {
            if let &Node::Present {
                ref parent,
                ..
            } = self.access_node_ref() {
                match parent.get() {
                    ParentId::Some { .. } => AboveMe::Parent,
                    ParentId::Root => AboveMe::Root,
                    ParentId::Detached => AboveMe::Detached,
                    ParentId::Garbage => unreachable!("encountered garbage parent outside of GC"),
                }
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    pub fn parent(&self) -> Result<Self, NoParent> {
        unsafe {
            if let &Node::Present {
                ref parent,
                ..
            } = self.access_node_ref() {
                match parent.get() {
                    ParentId::Some {
                        parent_index,
                        ..
                    } => Ok(Self::new(self.tree, parent_index)),
                    ParentId::Root => Err(NoParent::Root),
                    ParentId::Detached => Err(NoParent::Detached),
                    ParentId::Garbage => unreachable!("garbage parent node encountered outside of GC"),
                }
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    pub fn seek_parent(&mut self) -> Result<(), NoParent> {
        *self = self.parent()?;
        Ok(())
    }

    pub fn has_child(&self, branch: usize) -> Result<bool, InvalidBranchIndex> {
        unsafe {
            if let &Node::Present {
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

    pub fn child(&self, branch: usize) -> Result<Result<Self, ChildNotFound>, InvalidBranchIndex> {
        unsafe {
            if let &Node::Present {
                ref children,
                ..
            } = self.access_node_ref() {
                (&*children.get()).as_slice()
                    .get(branch)
                    .ok_or(InvalidBranchIndex(branch))
                    .map(|child_id| match child_id.index {
                        Some(child_index) => Ok(Self::new(self.tree, child_index)),
                        None => Err(ChildNotFound(branch)),
                    })
            } else {
                unreachable!("tree write traverser points to garbage node")
            }
        }
    }

    pub fn seek_child(&mut self, branch: usize) -> Result<Result<(), ChildNotFound>, InvalidBranchIndex> {
        match self.child(branch) {
            Ok(Ok(child)) => {
                *self = child;
                Ok(Ok(()))
            },
            Ok(Err(e)) => Ok(Err(e)),
            Err(e) => Err(e),
        }
    }

    pub fn this_branch_index(&self) -> Result<usize, NoParent> {
        unsafe {
            if let &Node::Present {
                ref parent,
                ..
            } = self.access_node_ref() {
                match parent.get() {
                    ParentId::Some {
                        this_branch,
                        ..
                    } => Ok(this_branch),
                    ParentId::Root => Err(NoParent::Root),
                    ParentId::Detached => Err(NoParent::Detached),
                    ParentId::Garbage => unreachable!("garbage parent node encountered outside of GC"),
                }
            } else {
                unreachable!("tree write traverser points to garbage")
            }
        }
    }

    unsafe fn access_node_ref(&self) -> &Node<T, C> {
        &*((&*self.tree.nodes.get())[self.index].get())
    }
}
impl<'t, T, C: FixedSizeArray<ChildId>> Deref for TreeReadTraverser<'t, T, C> {
    type Target = T;

    fn deref(&self) -> &T {
        self.elem
    }
}
impl<'s: 'tree, 'tree, T, C: FixedSizeArray<ChildId>> IntoReadGuard<'tree, T, C>
for &'s TreeReadTraverser<'tree, T, C> {
    fn into_read_guard(self) -> NodeReadGuard<'tree, T, C> {
        unsafe {
            NodeReadGuard::new(self.tree, self.index)
        }
    }
}

/// An opaque type which represents the index of a node in a tree. Created for the
/// traverse_from! and traverse_read_from! macros.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct NodeIndex {
    index: usize,
}

/// Given a TreeOperation and some type of guard which borrows from that tree and implements
/// IntoReadGuard, produce a TreeWriteTraverser that starts at that node.
///
/// For this to compile, called IntoReadGuard::into_read_guard on the second argument must
/// relinquish all borrowship of the TreeOperation. In other words, pass the node guard by
/// value, not reference.
#[macro_export]
macro_rules! traverse_from {
    ( $op:expr, $node:expr  ) => {{
        use bonzai::IntoReadGuard;
        let index = $node.into_read_guard().index();
        $op.traverse_from(index).unwrap()
    }}
}

/// Given a Tree and some type of guard which borrows from that tree and implements
/// IntoReadGuard, produce a TreeReadTraverser that starts at that node.
#[macro_export]
macro_rules! traverse_read_from {
    ( $op:expr, $node:expr  ) => {{
        use bonzai::IntoReadGuard;
        let index = $node.into_read_guard().index();
        $op.traverse_read_from(index).unwrap()
    }}
}