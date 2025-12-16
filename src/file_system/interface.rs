//! Implémentation d’un shell minimal pour naviguer dans un système FAT32
//!
//! Ce module fournit une interface de type *shell* permettant :
//! - de lister le contenu d’un répertoire (`ls`),
//! - de changer de répertoire (`cd`),
//! - d’afficher le contenu d’un fichier texte (`cat`).
//!
//! Il s’appuie sur [`Fat32FileSystem`] et les structures de haut niveau
//! [`FileInfo`] pour abstraire le format FAT32

use crate::{
    file_system::{list_directory_entries, Fat32FileSystem, FileInfo},
    print, println,
};
use alloc::{rc::Rc, string::ToString, vec::Vec};

/// Représente une session de shell FAT32.
///
/// Une session conserve
/// - une référence partagée vers le système de fichiers
/// - le cluster courant (équivalent du répertoire courant)
pub struct ShellSession {
    /// Système de fichiers FAT32 partagé
    fs: Rc<Fat32FileSystem>,

    /// Cluster courant (répertoire actif)
    pub current_cluster: u32,
}

impl ShellSession {
    /// Crée une nouvelle session de shell
    ///
    /// Le répertoire courant est initialisé au cluster racine
    pub fn new(fs: Rc<Fat32FileSystem>) -> ShellSession {
        let current_cluster = fs.root_cluster;
        ShellSession {
            fs,
            current_cluster,
        }
    }

    /// Liste le contenu d’un répertoire (`ls`)
    ///
    /// - Si `path` est `None`, liste le répertoire courant
    /// - Si `path` est fourni, liste le répertoire cible
    ///
    /// Les entrées spéciales `.` et `..` sont ignorées à l’affichage
    ///
    /// # Errors
    /// Retourne `"Entry not found"` si le chemin est invalide
    pub fn ls(&self, path: Option<&str>) -> Result<(), &str> {
        let cluster: u32;

        match path {
            Some(p) => {
                let file = self
                    .fs
                    .parse_path(p, Some(self.current_cluster))
                    .ok_or("Entry not found")?;

                cluster = file.start_cluster;
            }
            None => cluster = self.current_cluster,
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

    /// Change le répertoire courant (`cd`)
    ///
    /// Le chemin peut être :
    /// - absolu
    /// - relatif au répertoire courant
    ///
    /// # Errors
    /// - `"Entry not found"` si le chemin est invalide
    /// - `"Not a directory"` si la cible n’est pas un répertoire
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

    /// Retourne les entrées du répertoire courant
    ///
    /// Les entrées spéciales `.` et `..` sont filtrées
    pub fn ls_entries(&self) -> Vec<FileInfo> {
        list_directory_entries(&self.fs, self.current_cluster)
            .into_iter()
            .filter(|f| f.name != "." && f.name != "..")
            .collect()
    }

    /// Affiche le contenu d’un fichier (`cat`)
    ///
    /// Le contenu est affiché tel quel sur la sortie standard
    /// En cas d’erreur, le message est affiché à la place
    pub fn cat(&self, path: &str) -> Result<(), &str> {
        let data = match self.fs.read_file(path, None) {
            Ok(content) => content,
            Err(e) => e.to_string(),
        };

        println!("{}", data);
        Ok(())
    }
}
