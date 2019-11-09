use crate::diff::myers::{Edit, EditType};
use crate::pager::Pager;
use colored::*;
        Pager::setup_pager();

        writeln!(
            self.ctx.stdout,
            "{}",
            format!("diff --git {} {}", a.path, b.path).bold()
        );
                "{}",
                format!("new file mode {:o}", b.mode.expect("missing mode")).bold()
                "{}",
                format!("deleted file mode {:o}", a.mode.expect("missing mode")).bold()
                "{}",
                format!("old mode {:o}", a.mode.expect("missing mode")).bold()
                "{}",
                format!("new mode {:o}", b.mode.expect("missing mode")).bold()
            "{}",
            format!(
                "index {}..{}{}",
                short(&a.oid),
                short(&b.oid),
                if a.mode == b.mode {
                    format!(" {:o}", a.mode.expect("Missing mode"))
                } else {
                    format!("")
                }
            )
            .bold()
        writeln!(self.ctx.stdout, "{}", format!("--- {}", a.path).bold());
        writeln!(self.ctx.stdout, "{}", format!("+++ {}", b.path).bold());
    fn print_diff_edit(&mut self, edit: Edit) {
        let edit_string = match &edit.edit_type {
            &EditType::Ins => format!("{}", edit).green(),
            &EditType::Del => format!("{}", edit).red(),
            &EditType::Eql => format!("{}", edit).normal(),
        };
        writeln!(self.ctx.stdout, "{}", edit_string);
    }

        writeln!(self.ctx.stdout, "{}", hunk.header().cyan());
            self.print_diff_edit(edit);