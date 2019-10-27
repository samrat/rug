use crate::diff::Line;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditType {
    Eql,
    Ins,
    Del,
}

impl EditType {
    fn to_string(&self) -> &str {
        match self {
            EditType::Eql => " ",
            EditType::Ins => "+",
            EditType::Del => "-",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Edit {
    pub edit_type: EditType,
    pub a_line: Option<Line>,
    pub b_line: Option<Line>,
}

impl Edit {
    fn new(edit_type: EditType, a_line: Option<Line>, b_line: Option<Line>) -> Edit {
        Edit {
            edit_type,
            a_line,
            b_line,
        }
    }

}


impl fmt::Display for Edit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let line = if let Some(a) = &self.a_line {
            a
        } else if let Some(b) = &self.b_line {
            b
        } else {
            panic!("both lines None")
        };
        write!(f, "{}{}", self.edit_type.to_string(), line)
    }
}

pub struct Myers {
    a: Vec<Line>,
    b: Vec<Line>,
}

fn to_usize(i: isize) -> usize {
    usize::try_from(i).unwrap()
}

impl Myers {
    pub fn new(a: Vec<Line>, b: Vec<Line>) -> Myers {
        Myers { a, b }
    }

    pub fn diff(&self) -> Vec<Edit> {
        let mut diff = vec![];
        for (prev_x, prev_y, x, y) in self.backtrack().iter() {
            let a_line = if to_usize(*prev_x) >= self.a.len() {
                None
            } else {
                Some(self.a[to_usize(*prev_x)].clone())
            };

            let b_line = if to_usize(*prev_y) >= self.b.len() {
                None
            } else {
                Some(self.b[to_usize(*prev_y)].clone())
            };

            if x == prev_x {
                diff.push(Edit::new(EditType::Ins, None, b_line));
            } else if y == prev_y {
                diff.push(Edit::new(EditType::Del, a_line, None));
            } else {
                diff.push(Edit::new(EditType::Eql, a_line, b_line));
            }
        }

        diff.reverse();
        diff
    }

    fn shortest_edit(&self) -> Vec<BTreeMap<isize, isize>> {
        let n = self.a.len() as isize;
        let m = self.b.len() as isize;

        let max: isize = n + m;

        let mut v = BTreeMap::new();
        v.insert(1, 0);
        let mut trace = vec![];

        for d in 0..=max {
            trace.push(v.clone());
            for k in (-d..=d).step_by(2) {
                let mut x: isize =
                    if k == -d || (k != d && v.get(&(k - 1)).unwrap() < v.get(&(k + 1)).unwrap()) {
                        // v[k+1] has the farthest x- position along line
                        // k+1
                        // move downward
                        *v.get(&(k + 1)).unwrap()
                    } else {
                        // move rightward
                        v.get(&(k - 1)).unwrap() + 1
                    };

                let mut y: isize = x - k;
                while x < n && y < m && self.a[to_usize(x)].text == self.b[to_usize(y)].text {
                    x = x + 1;
                    y = y + 1;
                }

                v.insert(k, x);
                if x >= n && y >= m {
                    return trace;
                }
            }
        }
        vec![]
    }

    fn backtrack(&self) -> Vec<(isize, isize, isize, isize)> {
        let mut x = self.a.len() as isize;
        let mut y = self.b.len() as isize;
        let mut seq = vec![];

        for (d, v) in self.shortest_edit().iter().enumerate().rev() {
            let d = d as isize;
            let k = x - y;

            let prev_k =
                if k == -d || (k != d && v.get(&(k - 1)).unwrap() < v.get(&(k + 1)).unwrap()) {
                    k + 1
                } else {
                    k - 1
                };

            let prev_x = *v.get(&prev_k).unwrap();
            let prev_y = prev_x - prev_k;

            while x > prev_x && y > prev_y {
                seq.push((x - 1, y - 1, x, y));
                x = x - 1;
                y = y - 1;
            }

            if d > 0 {
                seq.push((prev_x, prev_y, x, y));
            }

            x = prev_x;
            y = prev_y;
        }

        seq
    }
}
