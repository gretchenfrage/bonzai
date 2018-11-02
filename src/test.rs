
use super::*;

#[test]
fn bad_test() {
    // the test here, is that this code shouldn't compile
    // if this code compiles, then bonzai is broken

    // example compile error:
    /*

error[E0499]: cannot borrow `op` as mutable more than once at a time
  --> bonzai\src\test.rs:16:17
   |
13 |     let mut a = op.write_root().unwrap();
   |                 -- first mutable borrow occurs here
...
16 |     let mut b = op.write_root().unwrap();
   |                 ^^ second mutable borrow occurs here
...
26 | }
   | - first borrow ends here

    */

    /*
    let mut tree = Tree::<i32, [ChildId; 2]>::new();
    let mut op = tree.operation();
    op.write_root();

    op.put_root_elem(0);

    op.write_root().unwrap();
    let mut a = op.write_root().unwrap();
    let ae = a.elem();

    let mut b = op.write_root().unwrap();
    let be = b.elem();

    println!("{}", ae);
    println!("{}", be);

    *ae += 1;

    println!("{}", ae);
    println!("{}", be);
    */
}