//! Implémentation minimale d’un lecteur FAT32 en lecture seule
//!
//! Ce module permet :
//! - de parser le secteur de boot FAT32
//! - de lire des secteurs et clusters
//! - de parcourir des répertoires
//! - de gérer les noms courts (8.3) et les Long File Names (LFN)
//! - de lire le contenu d’un fichier texte via son chemin
pub mod interface;

use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::{string::String, vec::Vec};

/// Représente un système de fichiers FAT32 monté en mémoire
#[derive(Debug, Clone)]
pub struct Fat32FileSystem {
    /// Disque brut monté en mémoire (image FAT32)
    pub disk: Box<[u8]>,

    /// Nombre d’octets par secteur
    pub bytes_per_sector: u32,

    /// Nombre de secteurs par cluster.
    pub sectors_per_cluster: u32,

    /// Premier secteur de la FAT.
    pub fat_sector: u32,

    /// Premier secteur de la zone de données.
    pub data_sector: u32,

    /// Cluster racine du système de fichiers.
    pub root_cluster: u32,
}

/// Offsets (en octets) dans le secteur de boot FAT32.
///
/// Ces valeurs sont définies par la spécification FAT32.
#[repr(usize)]
enum BootOffsets {
    BytsPerSec = 11,
    SecPerClus = 13,
    RsvdSecCnt = 14,
    NumFATs = 16,
    FATSz32 = 36,
    RootClus = 44,
}

impl Fat32FileSystem {
    /// Lit un entier 16 bits little-endian depuis le secteur de boot.
    fn read_u16(d: &[u8], off: BootOffsets) -> u16 {
        let o = off as usize;
        u16::from_le_bytes(d[o..o + 2].try_into().expect("Failed to read u16 data"))
    }

    /// Lit un entier 32 bits little-endian depuis le secteur de boot.
    fn read_u32(d: &[u8], off: BootOffsets) -> u32 {
        let o = off as usize;
        u32::from_le_bytes(d[o..o + 4].try_into().expect("Failed to read u32 data"))
    }

    /// Initialise un système de fichiers FAT32 à partir d’un disque brut.
    ///
    /// Cette fonction :
    /// - parse le secteur de boot,
    /// - calcule les offsets FAT et data,
    /// - identifie le cluster racine.
    pub fn new(disk: Box<[u8]>) -> Self {
        let bytes_per_sector = Self::read_u16(&disk, BootOffsets::BytsPerSec) as u32;
        let sectors_per_cluster = disk[BootOffsets::SecPerClus as usize] as u32;
        let reserved_sectors_count = Self::read_u16(&disk, BootOffsets::RsvdSecCnt) as u32;
        let num_fats = disk[BootOffsets::NumFATs as usize] as u32;
        let sectors_per_fat = Self::read_u32(&disk, BootOffsets::FATSz32);
        let root_cluster = Self::read_u32(&disk, BootOffsets::RootClus);

        let fat_sector = reserved_sectors_count;
        let data_sector = reserved_sectors_count + num_fats * sectors_per_fat;

        Fat32FileSystem {
            disk,
            bytes_per_sector,
            sectors_per_cluster,
            fat_sector,
            data_sector,
            root_cluster,
        }
    }

    /// Lit un secteur logique du disque.
    ///
    /// # Panics
    /// Panique si l’adresse dépasse la taille du disque.
    pub fn read_sector(&self, address: u32) -> Vec<u8> {
        let offset = (address * self.bytes_per_sector) as usize;
        let size = self.bytes_per_sector as usize;

        if offset + size > self.disk.len() {
            panic!("Error reading outbound");
        }

        self.disk[offset..offset + size].to_vec()
    }

    /// Lit un cluster complet (tous ses secteurs).
    pub fn read_cluster(&self, cluster_id: u32) -> Vec<u8> {
        let start_address = self.data_sector + (cluster_id - 2) * self.sectors_per_cluster;
        let mut data = Vec::new();

        for i in 0..self.sectors_per_cluster {
            let sector_data = self.read_sector(start_address + i);
            data.extend(sector_data);
        }

        data
    }

    /// Lit une entrée FAT pour obtenir le cluster suivant.
    ///
    /// Les bits de poids fort sont masqués conformément à la spécification FAT32.
    fn read_fat_entry(&self, cluster_id: u32) -> u32 {
        let fat_offset = cluster_id * 4;
        let fat_sector = self.fat_sector + fat_offset / self.bytes_per_sector;
        let fat_index = (fat_offset % self.bytes_per_sector) as usize;
        let sector = self.read_sector(fat_sector);

        let entry = u32::from_le_bytes(sector[fat_index..fat_index + 4].try_into().unwrap());
        entry & 0x0FFFFFFF
    }

    /// Lit le contenu d’un fichier texte à partir de son chemin.
    ///
    /// - Supporte les chemins absolus et relatifs
    /// - Gère les chaînes de clusters FAT
    ///
    /// # Errors
    /// - `"File not found"`
    /// - `"Not a file"`
    /// - `"Invalid UTF-8 content"`
    pub fn read_file(&self, path: &str, current_cluster: Option<u32>) -> Result<String, &str> {
        let file = self
            .parse_path(path, current_cluster)
            .ok_or("File not found")?;

        if file.is_directory {
            return Err("Not a file");
        }

        let mut data = Vec::new();
        let mut cluster = file.start_cluster;

        loop {
            data.extend(self.read_cluster(cluster));
            let next = self.read_fat_entry(cluster);

            if next >= 0x0FFFFFF8 {
                break;
            }

            cluster = next;
        }

        data.truncate(file.size as usize);
        String::from_utf8(data).map_err(|_| "Invalid UTF-8 content")
    }

    /// Résout un chemin en parcourant récursivement les répertoires.
    fn parse_path(&self, path: &str, current_cluster: Option<u32>) -> Option<FileInfo> {
        let mut cluster = if path.starts_with("/") {
            self.root_cluster
        } else {
            current_cluster.unwrap_or(self.root_cluster)
        };

        let parts: Vec<&str> = path.split("/").filter(|s| !s.is_empty()).collect();

        for (i, part) in parts.iter().enumerate() {
            let files = list_directory_entries(self, cluster);

            match *part {
                "." => continue,
                ".." => {
                    cluster = self.find_parent_cluster(cluster)?;
                    if i == parts.len() - 1 {
                        return Some(FileInfo::new("..".to_string(), true, 0, cluster));
                    }
                    continue;
                }
                _ => {}
            }

            let file = files.iter().find(|f| f.name == *part)?.clone();

            if i == parts.len() - 1 {
                return Some(file);
            }

            if !file.is_directory {
                return None;
            }

            cluster = file.start_cluster;
        }

        None
    }

    /// Recherche le cluster parent d’un répertoire via l’entrée `..`.
    fn find_parent_cluster(&self, current_cluster: u32) -> Option<u32> {
        if current_cluster == self.root_cluster {
            return None;
        }

        let files = list_directory_entries(self, current_cluster);
        let parent = files.iter().find(|f| f.name == "..")?;

        Some(if parent.start_cluster == 0 {
            self.root_cluster
        } else {
            parent.start_cluster
        })
    }
}

/// Représente une entrée de répertoire FAT32 standard (32 octets).
/// Cette structure correspond au layout sur disque d’une entrée FAT (format 8.3).
#[derive(Debug, Copy, Clone)]
pub struct FatDir {
    /// Nom court (8.3) encodé sur 11 octets.
    pub name: [u8; 11],

    /// Attributs FAT (directory, volume label, read-only, etc.).
    pub attr: u8,

    /// Partie haute du cluster de départ (FAT32).
    pub first_cluster_high: u16,

    /// Partie basse du cluster de départ.
    pub first_cluster_low: u16,

    /// Taille du fichier en octets (0 pour un répertoire).
    pub size: u32,
}

/// Offsets (en octets) dans une entrée de répertoire FAT.
/// Les offsets sont définis par la spécification FAT.
#[repr(usize)]
pub enum DirOffsets {
    /// Nom court (8.3).
    Name = 0,
    /// Attributs.
    Attr = 11,
    /// Partie haute du cluster de départ.
    FstClusHI = 20,
    /// Partie basse du cluster de départ.
    FstClusLO = 26,
    /// Taille du fichier.
    FileSize = 28,
}

impl FatDir {
    /// Lit un entier 16 bits little-endian depuis une entrée FAT.
    fn read_u16(data: &[u8], offset: DirOffsets) -> u16 {
        let o = offset as usize;
        u16::from_le_bytes(data[o..o + 2].try_into().unwrap())
    }

    /// Lit un entier 32 bits little-endian depuis une entrée FAT.
    fn read_u32(data: &[u8], offset: DirOffsets) -> u32 {
        let o = offset as usize;
        u32::from_le_bytes(data[o..o + 4].try_into().unwrap())
    }

    /// Construit une entrée [`FatDir`] à partir de 32 octets bruts.
    ///
    /// # Panics
    /// Panique si le buffer est trop petit.
    pub fn new(data: &[u8]) -> FatDir {
        let name = data[DirOffsets::Name as usize..DirOffsets::Name as usize + 11]
            .try_into()
            .unwrap();

        let attr = data[DirOffsets::Attr as usize];
        let first_cluster_high = Self::read_u16(data, DirOffsets::FstClusHI);
        let first_cluster_low = Self::read_u16(data, DirOffsets::FstClusLO);
        let size = Self::read_u32(data, DirOffsets::FileSize);

        FatDir {
            name,
            attr,
            first_cluster_high,
            first_cluster_low,
            size,
        }
    }
}

/// Représente une entrée Long File Name (LFN).
/// Les entrées LFN précèdent toujours l’entrée FAT classique correspondante et contiennent le nom en UTF-16.
pub struct LongFileName {
    /// Numéro de séquence (ordre inverse).
    pub seq_num: u8,

    /// Première partie du nom (5 caractères UTF-16).
    pub name_1: [u8; 10],

    /// Attribut LFN (toujours `0x0F`).
    pub attr: u8,

    /// Type LFN (toujours 0).
    pub l_type: u8,

    /// Checksum du nom court associé.
    pub chksum: u8,

    /// Deuxième partie du nom (6 caractères UTF-16).
    pub name_2: [u8; 12],

    /// Champ réservé.
    pub reserved_fch: u16,

    /// Troisième partie du nom (2 caractères UTF-16).
    pub name_3: [u8; 4],
}

/// Offsets (en octets) d’une entrée LFN.
#[repr(usize)]
pub enum LfnOffsets {
    Ord = 0,
    Name1 = 1,
    Attr = 11,
    LType = 12,
    ChkSum = 13,
    Name2 = 14,
    ReservedFCH = 26,
    Name3 = 28,
}

impl LongFileName {
    /// Construit une entrée LFN à partir de 32 octets bruts.
    ///
    /// # Panics
    /// Panique si les slices sont invalides.
    pub fn new(data: &[u8]) -> Self {
        let seq_num = data[LfnOffsets::Ord as usize];
        let attr = data[LfnOffsets::Attr as usize];
        let l_type = data[LfnOffsets::LType as usize];
        let chksum = data[LfnOffsets::ChkSum as usize];

        let name_1 = data[LfnOffsets::Name1 as usize..LfnOffsets::Name1 as usize + 10]
            .try_into()
            .expect("LFN Error: Failed to extract name_1");

        let name_2 = data[LfnOffsets::Name2 as usize..LfnOffsets::Name2 as usize + 12]
            .try_into()
            .expect("LFN Error: Failed to extract name_2");

        let name_3 = data[LfnOffsets::Name3 as usize..LfnOffsets::Name3 as usize + 4]
            .try_into()
            .expect("LFN Error: Failed to extract name_3");

        let reserved_fch = u16::from_le_bytes(
            data[LfnOffsets::ReservedFCH as usize..LfnOffsets::ReservedFCH as usize + 2]
                .try_into()
                .expect("LFN Error: Failed to extract reserved FCH"),
        );

        Self {
            seq_num,
            name_1,
            attr,
            l_type,
            chksum,
            name_2,
            reserved_fch,
            name_3,
        }
    }
}

/// Représente un fichier ou un répertoire au niveau logique.
/// Cette structure est indépendante du format FAT.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    /// Nom du fichier (LFN ou 8.3).
    pub name: String,

    /// Indique si l’entrée est un répertoire.
    pub is_directory: bool,

    /// Taille du fichier en octets.
    pub size: u32,

    /// Cluster de départ.
    pub start_cluster: u32,
}

impl FileInfo {
    /// Construit un nouvel objet [`FileInfo`].
    pub fn new(name: String, is_directory: bool, size: u32, start_cluster: u32) -> FileInfo {
        FileInfo {
            name,
            is_directory,
            size,
            start_cluster,
        }
    }
}

/// Calcule le checksum d’un nom court (8.3)
///
/// Ce checksum est utilisé par FAT pour lier une ou plusieurs entrées Long File Name (LFN) à l’entrée FAT classique correspondante
///
/// L’algorithme est défini par la spécification FAT
fn lfn_checksum(short_name: &[u8; 11]) -> u8 {
    let mut sum: u8 = 0;
    for &b in short_name.iter() {
        let carry = (sum & 1) << 7;
        sum = (sum >> 1).wrapping_add(carry).wrapping_add(b);
    }
    sum
}

/// Convertit un nom court FAT (8.3) en `String`
///
/// - Supprime les espaces de padding
/// - Gère l’extension
/// - Retourne un nom lisible (`FILE.TXT`)
fn short_name_to_string(name11: &[u8; 11]) -> String {
    let name_part = &name11[0..8];
    let ext_part = &name11[8..11];

    let name_str = {
        let mut end = name_part.len();
        while end > 0 && name_part[end - 1] == b' ' {
            end -= 1;
        }
        core::str::from_utf8(&name_part[..end])
            .unwrap_or("")
            .to_string()
    };

    let ext_str = {
        let mut end = ext_part.len();
        while end > 0 && ext_part[end - 1] == b' ' {
            end -= 1;
        }
        core::str::from_utf8(&ext_part[..end])
            .unwrap_or("")
            .to_string()
    };

    if ext_str.is_empty() {
        name_str
    } else {
        let mut s = name_str;
        s.push('.');
        s.push_str(&ext_str);
        s
    }
}

/// Convertit un fragment de bytes LFN en UTF-16 (`u16`)
///
/// Les champs LFN sont stockés en little-endian sur 2 octets
fn byte_to_u16_vec(fragment: &[u8]) -> Vec<u16> {
    fragment
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

/// Fragments LFN collectés avant l’entrée FAT classique
///
/// - `u8` : numéro de séquence
/// - `Vec<u16>` : caractères UTF-16
type LfnFragments = Vec<(u8, Vec<u16>)>;

/// Liste les entrées d’un répertoire FAT32.
///
/// Cette fonction :
/// - parcourt les entrées de 32 octets
/// - gère les entrées supprimées et de fin
/// - reconstruit les noms longs (LFN)
/// - retourne une liste de [`FileInfo`]
pub fn list_directory_entries(fs: &Fat32FileSystem, cluster_id: u32) -> Vec<FileInfo> {
    let cluster_data = fs.read_cluster(cluster_id);
    let mut results = Vec::new();

    let mut lfn_fragments: LfnFragments = Vec::new();
    let mut expected_checksum: Option<u8> = None;

    const ENTRY_SIZE: usize = 32;
    const ATTR_LFN: u8 = 0x0F;
    const ATTR_DIRECTORY: u8 = 0x10;

    for entry_chunk in cluster_data.chunks_exact(ENTRY_SIZE) {
        let first_byte = entry_chunk[0];
        let attributes = entry_chunk[11];

        // Fin des entrées
        if first_byte == 0x00 {
            break;
        }

        // Entrée supprimée
        if first_byte == 0xE5 {
            lfn_fragments.clear();
            expected_checksum = None;
            continue;
        }

        // Entrée LFN
        if attributes == ATTR_LFN {
            process_lfn_entry(entry_chunk, &mut lfn_fragments, &mut expected_checksum);
        } else {
            // Entrée FAT classique
            if let Some(file_info) = process_data_entry(
                entry_chunk,
                &mut lfn_fragments,
                &mut expected_checksum,
                ATTR_DIRECTORY,
            ) {
                results.push(file_info);
            }

            lfn_fragments.clear();
            expected_checksum = None;
        }
    }

    results
}

/// Traite une entrée Long File Name (LFN).
///
/// Les fragments sont stockés temporairement jusqu’à
/// la rencontre de l’entrée FAT correspondante
fn process_lfn_entry(
    entry_chunk: &[u8],
    lfn_fragments: &mut LfnFragments,
    expected_checksum: &mut Option<u8>,
) {
    let lfn_entry = LongFileName::new(entry_chunk);

    let seq = lfn_entry.seq_num & 0x1F;
    let is_last = (lfn_entry.seq_num & 0x40) != 0;

    if is_last {
        lfn_fragments.clear();
        *expected_checksum = Some(lfn_entry.chksum);
    }

    let mut fragment_data: Vec<u16> = Vec::new();
    fragment_data.extend(byte_to_u16_vec(&lfn_entry.name_1));
    fragment_data.extend(byte_to_u16_vec(&lfn_entry.name_2));
    fragment_data.extend(byte_to_u16_vec(&lfn_entry.name_3));

    let mut replaced = false;
    for (existing_seq, existing_frag) in lfn_fragments.iter_mut() {
        if *existing_seq == seq {
            *existing_frag = fragment_data.clone();
            replaced = true;
            break;
        }
    }

    if !replaced {
        lfn_fragments.push((seq, fragment_data));
    }
}

/// Assemble les fragments LFN en une `String` UTF-8
///
/// - Trie les fragments par numéro de séquence
/// - Ignore les caractères de fin (`0x0000`, `0xFFFF`)
/// - Convertit UTF-16 → UTF-8
fn assemble_lfn(lfn_fragments: &LfnFragments) -> Option<String> {
    if lfn_fragments.is_empty() {
        return None;
    }

    let mut frags = lfn_fragments.clone();
    frags.sort_by(|a, b| a.0.cmp(&b.0));

    let mut utf16_chars: Vec<u16> = Vec::new();
    for (_seq, frag) in frags {
        for &ch in frag.iter() {
            if ch == 0x0000 || ch == 0xFFFF {
                if ch == 0x0000 {
                    break;
                }
                continue;
            }
            utf16_chars.push(ch);
        }
    }

    String::from_utf16(&utf16_chars).ok()
}

/// Traite une entrée FAT classique et construit un [`FileInfo`]
///
/// - Vérifie le checksum LFN
/// - Détermine le type (fichier ou répertoire)
/// - Calcule le cluster de départ
fn process_data_entry(
    entry_chunk: &[u8],
    lfn_fragments: &mut LfnFragments,
    expected_checksum: &mut Option<u8>,
    attr_directory_mask: u8,
) -> Option<FileInfo> {
    let dir_entry = FatDir::new(entry_chunk);

    // Volume label
    if dir_entry.attr & 0x08 != 0 {
        return None;
    }

    let start_cluster =
        ((dir_entry.first_cluster_high as u32) << 16) | (dir_entry.first_cluster_low as u32);

    let is_directory = (dir_entry.attr & attr_directory_mask) != 0;
    let size = dir_entry.size;

    let mut name_to_use: Option<String> = None;

    // Tentative de reconstruction LFN
    if !lfn_fragments.is_empty() {
        let computed = lfn_checksum(&dir_entry.name);
        if let Some(expected) = expected_checksum {
            if *expected == computed {
                name_to_use = assemble_lfn(&lfn_fragments);
            } else {
                lfn_fragments.clear();
            }
        } else {
            lfn_fragments.clear();
        }
    }

    // Fallback si nom court
    if name_to_use.is_none() {
        name_to_use = Some(short_name_to_string(&dir_entry.name));
    }

    Some(FileInfo::new(
        name_to_use.unwrap_or_default(),
        is_directory,
        size,
        start_cluster,
    ))
}

/// Retourne uniquement les noms des fichiers
///
/// Fonction utilitaire pour affichage ou debug
pub fn list_files_names<'a>(files: &'a [FileInfo]) -> Vec<&'a str> {
    files.iter().map(|f| f.name.as_str()).collect()
}
