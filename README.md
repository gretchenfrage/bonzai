# Bonzai

Part of Rust's impressive performance comes from Rust storing data in-place by default, which 
improves spatial locality, causing the CPU-cache to perform better. Trees, however, often 
miss out on this benefit. Trees are recursive structures, so they need to use a form of 
indirection. People tend to default to the `Box`, since it has the same ownership semantics 
as normal, owned data. However, it has the potential to splay data all over the heap, killing
spatial locality.

One technique for resolving this issue is storing a tree's nodes in a contiguous `Vec`, and 
connecting them with indices. The most direct approach is to implement indexing logic in a way 
that is highly coupled with the logic of the tree itself. Unfortunately, this approach bears
a striking resemblance to dealing with raw pointers, both in the bugs it can allow and the 
technical complexity is creates. 

Bonzai is a safe, zero-cost abstraction over trees which store their nodes in a contiguous 
`Vec`. Bonzai uses unsafe code to create a safe abstraction which leverages the borrow checker 
to verify the validity of operations on trees, while using very few heap allocations. 

### Features:

- Entirely safe interface
- Minimal runtime cost
- Improved pointer aliasing
- Multiple simultaneous mutable references to different parts of tree
- Traversing from nodes to their parents
- Detach a subtree, reattach it somewhere else
- Compaction of nodes and re-shrinking of memory footprint
- Tree is `Send` and `Sync` if element is
- Compile-time generic over branch factor
- Pretty-printing trees through `Debug` trait

### Unsupported at this time:

- Multithreaded mutation
- Use on stable release channel
- Unbounded/dynamic branch factor
- Two-way traversal of detached subtree

### Example, performance test

The [bonzai-nbst](https://github.com/gretchenfrage/bonzai-nbst) repository is a rust executable which contains implementations of a 
naive binary search tree (no balancing), using bonzai in one, and heap allocations in the other.
[The bonzai-based implementation.](https://github.com/gretchenfrage/bonzai-nbst/blob/master/src/bst/bonzai.rs)

I tested the two implementations with 10,000,000 random tree operations, on a windows 10 laptop. To test it in a real-world scenario with 
chaotic heap use, I plugged it into a fork of the [magog roguelike](https://github.com/rsaarelm/magog) (which I am not associated 
with), such that they run in the same process. While this test is very unscientific, bonzai demonstrated a clear performance
improvement:

- bonzai: 15,786 ms
- boxes: 21,338 ms


### Tree<T, C>

The `Tree`, and nearly all components borrowed from the tree, is generic over two types: 
the element type, and a sized array of `ChildId`. The element type, `T`, is stored for 
every node in the tree. Each node may have any combination of children, with any child 
index that is a valid index of the `ChildId` array. 

For example, a binary search tree set of `i32` could be represented as a `Tree<i32, [ChildId; 2]>`.

### TreeOperation

A `TreeOperation` is a type which borrows mutably from the `Tree`, and allows for modification to that 
tree. While a `TreeOperation` exists, the access to the tree can only be single-threaded. This allows 
many operations to mutate the tree with only a immutable reference to the `TreeOperation`, directly or
indirectly.

### NodeOwnedGuard

Normally, a node in a tree is owned by its parent. The root node has a special status, where its parent
is considered to be root. However, during the lifetime of a `TreeOperation`, a subtree can be detached
from its parent, and it can be considered to be owned by a `NodeOwnedGuard`. In this case, the 
`NodeOwnedGuard` can outlive the guard for its parent. This subtree can be reattached to the main tree 
as the child of another node. During the lifetime of the `NodeOwnedGuard`, it is possible to mutate it,
as if it were part of the main tree. Alternatively, a `NodeOwnedGuard` can be transformed into its 
inner element, marking the node and all its children as garbage.

When a `NodeOwnedGuard` is dropped, and its subtree is marked as garbage, the elements' destructors will 
run sometime between the dropping of the `NodeOwnedGuard` and the dropping of the `TreeOperation`.

### NodeWriteGuard

A `NodeWriteGuard` is a type which holds mutable access to a subset of the tree (a node and all its 
children, recursively). The `NodeWriteGuard` can be simultaneously borrowed into a mutable reference 
to the element (`&mut T`) and a `ChildWriteGuard`. The `ChildWriteGuard` has the ability to alter the 
node's children. Additionally, a `NodeWriteGuard` can be turned into a `NodeOwnedGuard`, detaching the 
guarded subtree.

A `NodeWriteGuard` cannot outlive the parent node guard (if the node is root, it cannot
outlive the tree). 

### ChildWriteGuard

A `ChildWriteGuard` represents mutable access to a node's children. It can be borrowed from both a 
`NodeWriteGuard` and a `NodeOwnedGuard`. The `ChildWriteGuard` can borrow out a `NodeWriteGuard` to 
its child, or even several different children simultaneously. Additionally, a `ChildWriteGuard` can 
put an element as a particular child (marking any previous child subtree as garbage), or even attach 
an entire detached subtree (a `NodeOwnedGuard`) as one of its children.

### TreeWriteTraverser

While a `NodeWriteGuard` holds mutable access to a subtree (a node and its children, recursively), a
`TreeWriteTraverser` holds mutable access to an entire tree. This allows a `TreeWriteTraverser` to 
seek the parent node, which a `NodeWriteGuard` cannot do. However, this also means that a 
`TreeWriteTraverser` cannot exist at the same time as any other guards.

Beyond that difference, a `TreeWriteTraverser` can be used similarly to a `NodeWriteGuard`.
A `TreeWriteTraverser` can even be turned or mutably borrowed into a `NodeWriteGuard`.

A `TreeOperation` and some type of node guard can be conveniently turned into a `TreeWriteTraverser`
with the `traverse_from!` macro.

### NodeReadGuard

A `NodeReadGuard` is a type which holds immutable access to a subset of the tree. Because a 
`NodeReadGuard` doesn't mutably borrow anything, it does not need to split into a `ChildWriteGuard`
and `&mut T`. Instead, the `NodeReadGuard` immutably dereferences to a `T`, and its children 
can be accessed directly with a method.

### TreeReadTraverser

The `TreeReadTraverser` is to the `NodeReadGuard` as the `TreeWriteTraverser` is to the `NodeWriteGuard`.
While the `NodeReadGuard` holds immutable access to a subset of the tree, a `TreeReadTraverser` holds 
immutable access to the entire tree. This allows the `TreeReadTraverser` to safely traverse to 
its parent node.