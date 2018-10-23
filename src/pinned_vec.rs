
use std::ops::{Index, IndexMut};
use std::fmt::{Debug, Formatter};
use std::fmt;
use std::iter::{Iterator, IntoIterator};
use std::slice::Iter;

pub struct PinnedVec<T> {
    vec: Vec<T>,
    next: Option<Box<PinnedVec<T>>>,
    extension_size: usize,
}

impl<T> PinnedVec<T> {
    pub fn compress(&mut self) {
        let mut option_curr_link = self.next.take();
        while let Some(curr_link) = option_curr_link {
            let curr_link = *curr_link;
            let PinnedVec {
                vec: curr_vec,
                next: curr_next,
                ..
            } = curr_link;
            self.vec.extend(curr_vec.into_iter());
            option_curr_link = curr_next;
        }
    }

    pub fn new(extension_size: usize) -> Self {
        PinnedVec {
            vec: Vec::with_capacity(extension_size),
            next: None,
            extension_size
        }
    }

    pub fn push(&mut self, elem: T) {
        if self.vec.len() < self.vec.capacity() {
            self.vec.push(elem);
        } else {
            match &mut self.next {
                &mut Some(ref mut next) => {
                    next.push(elem);
                },
                none => {
                    let mut next = Box::new(PinnedVec {
                        vec: Vec::with_capacity(self.extension_size),
                        next: None,
                        extension_size: self.extension_size
                    });
                    next.push(elem);
                    *none = Some(next);
                }
            };
        }
    }

    pub fn len(&self) -> usize {
        self.vec.len() + self.next.as_ref()
            .map(|next| next.len())
            .unwrap_or(0)
    }

    pub fn pop(&mut self) -> Option<T> {
        if let Some(ref mut next) = self.next {
            next.pop()
        } else {
            self.vec.pop()
        }
    }

    pub fn swap_remove(&mut self, index: usize) {
        match self.len() {
            0 => {
                // this is illegal
                panic!("swap remove on empty PinnedVec");
            },
            len if len - 1 == index => {
                // if we're swap-removing the last element, simply pop it
                self.pop().unwrap();
            },
            _ => {
                // if we're swap-removing some middle element, actually swap remove
                self[index] = self.pop().unwrap();
            }
        }
    }

    pub fn iter<'a>(&'a self) -> PinnedVecIter<'a, T> {
        PinnedVecIter {
            curr: Some((self, self.vec.iter()))
        }
    }
}

impl<T> Index<usize> for PinnedVec<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        if index < self.vec.capacity() {
            &self.vec[index]
        } else {
            debug_assert_eq!(self.vec.len(), self.vec.capacity(),
                             "PinnedVec index out of bounds {}", index);
            self.next.as_ref()
                .ok_or_else(|| format!("PinnedVec index out of bounds {}", index)).unwrap()
                .index(index - self.vec.len())
        }
    }
}
impl<T> IndexMut<usize> for PinnedVec<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        if index < self.vec.capacity() {
            &mut self.vec[index]
        } else {
            debug_assert_eq!(self.vec.len(), self.vec.capacity(),
                             "PinnedVec index out of bounds {}", index);
            self.next.as_mut()
                .ok_or_else(|| format!("PinnedVec index out of bounds {}", index)).unwrap()
                .index_mut(index - self.vec.len())
        }
    }
}

impl<T: Debug> Debug for PinnedVec<T> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let mut builder = f.debug_list();
        let mut option_curr = Some(self);
        while let Some(curr) = option_curr {
            for elem in &curr.vec {
                builder.entry(elem);
            }
            option_curr = curr.next.as_ref().map(|next| &**next);
        }
        builder.finish()
    }
}

pub struct PinnedVecIter<'a, T> {
    curr: Option<(&'a PinnedVec<T>, Iter<'a, T>)>,
}
impl<'a, T> Iterator for PinnedVecIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        if self.curr.is_some() {
            if let Some(next) = self.curr.as_mut().unwrap().1.next() {
                Some(next)
            } else {
                self.curr = self.curr.as_ref().unwrap().0
                    .next.as_ref()
                    .map(|next_link| (&**next_link, next_link.vec.iter()));
                self.next()
            }
        } else {
            None
        }
    }
}