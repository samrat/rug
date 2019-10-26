mod myers;
use myers::{Myers, Edit};

pub struct Diff {
}

impl Diff {
    pub fn diff(a: &str, b: &str) -> Vec<Edit> {
        let a_lines : Vec<&str> = a.split('\n').collect();
        let b_lines : Vec<&str> = b.split('\n').collect();
        
        Myers::new(a_lines, b_lines).diff()
    }
}
