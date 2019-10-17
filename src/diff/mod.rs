mod myers;
use myers::{Myers, Edit};

pub struct Diff {
}

impl Diff {
    pub fn diff(a: Vec<&str>, b: Vec<&str>) -> Vec<Edit> {
        Myers::new(a, b).diff()
    }
}
