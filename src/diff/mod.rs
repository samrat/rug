mod myers;
use myers::{Edit, EditType, Myers};
use std::fmt;

pub struct Diff {}

#[derive(Clone, Debug)]
pub struct Line {
    number: usize,
    text: String,
}

impl Line {
    fn new(number: usize, text: &str) -> Line {
        Line {
            number,
            text: text.to_string(),
        }
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text)
    }
}

fn lines(a: &str) -> Vec<Line> {
    let mut a_lines = vec![];
    for (i, text) in a.split('\n').enumerate() {
        a_lines.push(Line::new(i + 1, text));
    }

    a_lines
}

impl Diff {
    pub fn diff(a: &str, b: &str) -> Vec<Edit> {
        let a_lines = lines(a);
        let b_lines = lines(b);

        Myers::new(a_lines, b_lines).diff()
    }

    pub fn diff_hunks(a: &str, b: &str) -> Vec<Hunk> {
        Hunk::filter(Self::diff(a, b))
    }
}

fn get_edit(edits: &[Edit], offset: isize) -> Option<&Edit> {
    if offset < 0 || offset >= edits.len() as isize {
        None
    } else {
        Some(&edits[offset as usize])
    }
}

const HUNK_CONTEXT: usize = 3;

const empty_edit: Edit = Edit {
    edit_type: EditType::Eql,
    a_line: None,
    b_line: None,
};

pub struct Hunk {
    pub a_start: usize,
    pub b_start: usize,
    pub edits: Vec<Edit>,
}

enum LineType {
    A,
    B,
}

impl Hunk {
    fn new(a_start: usize, b_start: usize, edits: Vec<Edit>) -> Hunk {
        Hunk {
            a_start,
            b_start,
            edits,
        }
    }

    pub fn header(&self) -> String {
        let (a_start, a_lines) = self.offsets_for(LineType::A, self.a_start);
        let (b_start, b_lines) = self.offsets_for(LineType::B, self.b_start);

        format!("@@ -{},{} +{},{} @@", a_start, a_lines, b_start, b_lines)
    }

    fn offsets_for(&self, line_type: LineType, default: usize) -> (usize, usize) {
        let lines: Vec<_> = self
            .edits
            .iter()
            .map(|e| match line_type {
                LineType::A => &e.a_line,
                LineType::B => &e.b_line,
            })
            .filter_map(|l| l.as_ref())
            .collect();
        let start = if lines.len() > 0 {
            lines[0].number
        } else {
            default
        };

        (start, lines.len())
    }

    pub fn filter(edits: Vec<Edit>) -> Vec<Hunk> {
        let mut hunks = vec![];
        let mut offset: isize = 0;

        let empty_line = Line::new(0, "");

        loop {
            loop {
                // Skip over Eql edits
                if let Some(edit) = get_edit(&edits, offset) {
                    if edit.edit_type == EditType::Eql {
                        offset += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            if offset >= (edits.len() as isize) {
                return hunks;
            }

            offset -= (HUNK_CONTEXT + 1) as isize;

            let a_start = if offset < 0 {
                0
            } else {
                get_edit(&edits, offset)
                    .unwrap_or(&empty_edit)
                    .a_line
                    .clone()
                    .unwrap_or(empty_line.clone())
                    .number
            };

            let b_start = if offset < 0 {
                0
            } else {
                get_edit(&edits, offset)
                    .unwrap_or(&empty_edit)
                    .b_line
                    .clone()
                    .unwrap_or(empty_line.clone())
                    .number
            };

            let (hunk, new_offset) = Self::build_hunk(a_start, b_start, &edits, offset);
            hunks.push(hunk);
            offset = new_offset;
        }
        hunks
    }

    fn build_hunk(
        a_start: usize,
        b_start: usize,
        edits: &[Edit],
        mut offset: isize,
    ) -> (Hunk, isize) {
        let mut counter: isize = -1;

        let mut hunk = Hunk::new(a_start, b_start, vec![]);

        while counter != 0 {
            if offset >= 0 && counter > 0 {
                hunk.edits.push(
                    get_edit(edits, offset)
                        .expect("offset out of bounds")
                        .clone(),
                )
            }

            offset += 1;
            if offset >= edits.len() as isize {
                break;
            }

            if let Some(edit) = get_edit(edits, offset + HUNK_CONTEXT as isize) {
                match edit.edit_type {
                    EditType::Ins | EditType::Ins => {
                        counter = (2 * HUNK_CONTEXT + 1) as isize;
                    }
                    _ => {
                        counter -= 1;
                    }
                }
            } else {
                counter -= 1;
            }
        }

        (hunk, offset)
    }
}
