use alloc::{rc::Rc, vec::Vec};
use crate::{file_system::{Fat32FileSystem, FileInfo, list_directory_entries}, print};

pub struct ShellSession {
    fs: Rc<Fat32FileSystem>,
    pub current_cluster: u32,
}

impl ShellSession {
    pub fn new(fs: Rc<Fat32FileSystem>) -> ShellSession {
        let current_cluster = fs.root_cluster;
        ShellSession {
            fs,
            current_cluster,
        }
    }

    pub fn ls(&self, path: Option<&str>) -> Result<(), &str> {
        let cluster: u32;
        match path {
            Some(p) => {
                let file = self
                    .fs
                    .parse_path(p, Some(self.current_cluster))
                    .ok_or("Entry not found")?;
                cluster = file.start_cluster;
            },
            None => cluster = self.current_cluster
        }

        let files = list_directory_entries(&self.fs, cluster);

        print!("> ");
        for f in files.iter() {
            if f.name == "." || f.name == ".." {
                continue;
            }

            let file_type = if f.is_directory { "[DIR]" } else { "[FILE]" };

            print!("{} {} ", file_type, f.name);
        }
        print!("\n");

        Ok(())
    }

    pub fn cd(&mut self, path: &str) -> Result<(), &str> {
        let file = self
            .fs
            .parse_path(path, Some(self.current_cluster))
            .ok_or("Entry not found")?;

        if !file.is_directory {
            return Err("Not a directory");
        }

        self.current_cluster = file.start_cluster;

        Ok(())
    }

    pub fn ls_entries(&self) -> Vec<FileInfo> {
        list_directory_entries(&self.fs, self.current_cluster)
            .into_iter()
            .filter(|f| f.name != "." && f.name != "..")
            .collect()
    }
}