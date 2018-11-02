
use super::*;

#[test]
fn bad_test() {
    let mut tree = Tree::<i32, [ChildId; 2]>::new();
    let mut op = tree.operation();
    op.put_root_elem(0);

    let mut a = op.write_root().unwrap();
    let ae = a.elem();
    let mut b = op.write_root().unwrap();
    let be = b.elem();

    println!("{}", ae);
    println!("{}", be);

    *ae += 1;

    println!("{}", ae);
    println!("{}", be);
}