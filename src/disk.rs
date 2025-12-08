use alloc::rc::Rc;
use alloc::string::ToString;
use alloc::{string::String, vec::Vec};

use crate::print;

#[derive(Debug, Copy, Clone)]
pub struct Fat32FileSystem {
    pub disk: &'static [u8],

    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,

    pub fat_sector: u32,
    pub data_sector: u32,

    pub root_cluster: u32,
}

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
    fn read_u16(d: &[u8], off: BootOffsets) -> u16 {
        let o = off as usize;
        u16::from_le_bytes(d[o..o + 2].try_into().expect("Failed to read u16 data"))
    }

    fn read_u32(d: &[u8], off: BootOffsets) -> u32 {
        let o = off as usize;
        u32::from_le_bytes(d[o..o + 4].try_into().expect("Failed to read u32 data"))
    }

    pub fn new(disk: &'static [u8]) -> Self {
        let bytes_per_sector = Self::read_u16(disk, BootOffsets::BytsPerSec) as u32;
        let sectors_per_cluster = disk[BootOffsets::SecPerClus as usize] as u32;
        let reserved_sectors_count = Self::read_u16(disk, BootOffsets::RsvdSecCnt) as u32;
        let num_fats = disk[BootOffsets::NumFATs as usize] as u32;
        let sectors_per_fat = Self::read_u32(disk, BootOffsets::FATSz32);
        let root_cluster = Self::read_u32(disk, BootOffsets::RootClus);

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

    pub fn read_sector(&self, address: u32) -> Vec<u8> {
        let offset = (address * self.bytes_per_sector) as usize;

        let size = self.bytes_per_sector as usize;

        if offset + size > self.disk.len() {
            panic!("Error reading outbound");
        }

        self.disk[offset..offset + size].to_vec()
    }

    pub fn read_cluster(&self, cluster_id: u32) -> Vec<u8> {
        let start_address = self.data_sector + (cluster_id - 2) * self.sectors_per_cluster;

        let mut data = Vec::new();

        for i in 0..self.sectors_per_cluster {
            let address_to_read = start_address + i;
            let sector_data = self.read_sector(address_to_read);
            data.extend(sector_data);
        }

        data
    }

    fn read_fat_entry(&self, cluster_id: u32) -> u32 {
        let fat_offset = cluster_id * 4;
        let fat_sector = self.fat_sector + fat_offset / self.bytes_per_sector;
        let fat_index = (fat_offset % self.bytes_per_sector) as usize;
        let sector = self.read_sector(fat_sector);

        let entry = u32::from_le_bytes(sector[fat_index..fat_index + 4].try_into().unwrap());

        entry & 0x0FFFFFFF
    }

    pub fn read_file(&self, path: &str, current_cluster: Option<u32>) -> Result<String, &str> {
        let file = self
            .parse_path(path, current_cluster)
            .ok_or("File not found")?;

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
                    if let Some(parent_cluster) = self.find_parent_cluster(cluster) {
                        cluster = parent_cluster;
                    } else {
                        return None;
                    }
                    continue;
                }
                _ => {}
            }

            let file_opt = files.iter().find(|f| f.name == *part);
            let file = match file_opt {
                Some(f) => f.clone(),
                None => return None,
            };

            if i == parts.len() - 1 {
                return Some(file);
            } else {
                if !file.is_directory {
                    return None;
                }
                cluster = file.start_cluster;
            }
        }

        None
    }

    fn find_parent_cluster(&self, current_cluster: u32) -> Option<u32> {
        if current_cluster == self.root_cluster {
            return None;
        }

        let files = list_directory_entries(self, current_cluster);

        if let Some(parent_dir_entry) = files.iter().find(|f| f.name == "..") {
            let parent_cluster = parent_dir_entry.start_cluster;

            if parent_cluster == 0 {
                return Some(self.root_cluster);
            }

            return Some(parent_cluster);
        }

        None
    }
}

#[derive(Debug, Copy, Clone)]
pub struct FatDir {
    pub name: [u8; 11],
    pub attr: u8,
    pub first_cluster_high: u16,
    pub first_cluster_low: u16,
    pub size: u32,
}

#[repr(usize)]
pub enum DirOffsets {
    Name = 0,
    Attr = 11,
    FstClusHI = 20,
    FstClusLO = 26,
    FileSize = 28,
}

impl FatDir {
    fn read_u16(data: &[u8], offset: DirOffsets) -> u16 {
        let o = offset as usize;
        u16::from_le_bytes(data[o..o + 2].try_into().unwrap())
    }

    fn read_u32(data: &[u8], offset: DirOffsets) -> u32 {
        let o = offset as usize;
        u32::from_le_bytes(data[o..o + 4].try_into().unwrap())
    }

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

pub struct LongFileName {
    pub seq_num: u8,
    pub name_1: [u8; 10],
    pub attr: u8,
    pub l_type: u8,
    pub chksum: u8,
    pub name_2: [u8; 12],
    pub reserved_fch: u16,
    pub name_3: [u8; 4],
}

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
    pub fn new(data: &[u8]) -> Self {
        let seq_num = data[LfnOffsets::Ord as usize];
        let attr = data[LfnOffsets::Attr as usize];
        let l_type = data[LfnOffsets::LType as usize];
        let chksum = data[LfnOffsets::ChkSum as usize];

        let name_1: [u8; 10] = data[LfnOffsets::Name1 as usize..LfnOffsets::Name1 as usize + 10]
            .try_into()
            .expect("LFN Error: Failed to extract name_1 (10 bytes)");

        let name_2: [u8; 12] = data[LfnOffsets::Name2 as usize..LfnOffsets::Name2 as usize + 12]
            .try_into()
            .expect("LFN Error: Failed to extract name_2 (12 bytes)");

        let name_3: [u8; 4] = data[LfnOffsets::Name3 as usize..LfnOffsets::Name3 as usize + 4]
            .try_into()
            .expect("LFN Error: Failed to extract name_3 (4 bytes)");

        let reserved_fch = u16::from_le_bytes(
            data[LfnOffsets::ReservedFCH as usize..LfnOffsets::ReservedFCH as usize + 2]
                .try_into()
                .expect("LFN Error: Failed to extract reserved FCH (2 bytes)"),
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

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: String,
    pub is_directory: bool,
    pub size: u32,
    pub start_cluster: u32,
}

impl FileInfo {
    pub fn new(name: String, is_directory: bool, size: u32, start_cluster: u32) -> FileInfo {
        FileInfo {
            name,
            is_directory,
            size,
            start_cluster,
        }
    }
}

fn lfn_checksum(short_name: &[u8; 11]) -> u8 {
    let mut sum: u8 = 0;
    for &b in short_name.iter() {
        let carry = (sum & 1) << 7;
        sum = (sum >> 1).wrapping_add(carry).wrapping_add(b);
    }
    sum
}

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

fn byte_to_u16_vec(fragment: &[u8]) -> Vec<u16> {
    fragment
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

type LfnFragments = Vec<(u8, Vec<u16>)>;

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

        if first_byte == 0x00 {
            break;
        }
        if first_byte == 0xE5 {
            lfn_fragments.clear();
            expected_checksum = None;
            continue;
        }

        if attributes == ATTR_LFN {
            process_lfn_entry(entry_chunk, &mut lfn_fragments, &mut expected_checksum);
        } else {
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

fn assemble_lfn(lfn_fragments: &LfnFragments) -> Option<String> {
    if lfn_fragments.is_empty() {
        return None;
    }
    let mut frags = lfn_fragments.clone();
    frags.sort_by(|a, b| {
        let (sa, _) = a;
        let (sb, _) = b;
        sa.cmp(sb)
    });

    let mut utf16_chars: Vec<u16> = Vec::new();
    for (_seq, frag) in frags {
        for &ch in frag.iter() {
            if ch == 0x0000 || ch == 0xFFFF {
                if ch == 0x0000 {
                    break;
                } else {
                    continue;
                }
            }
            utf16_chars.push(ch);
        }
    }

    match String::from_utf16(&utf16_chars) {
        Ok(s) => Some(s),
        Err(_) => None,
    }
}

fn process_data_entry(
    entry_chunk: &[u8],
    lfn_fragments: &mut LfnFragments,
    expected_checksum: &mut Option<u8>,
    attr_directory_mask: u8,
) -> Option<FileInfo> {
    let dir_entry = FatDir::new(entry_chunk);

    if dir_entry.attr & 0x08 != 0 {
        return None;
    }

    let start_cluster =
        ((dir_entry.first_cluster_high as u32) << 16) | (dir_entry.first_cluster_low as u32);

    let is_directory = (dir_entry.attr & attr_directory_mask) != 0;
    let size = dir_entry.size;

    let mut name_to_use: Option<String> = None;

    if !lfn_fragments.is_empty() {
        let computed = lfn_checksum(&dir_entry.name);
        if let Some(expected) = expected_checksum {
            if *expected == computed {
                if let Some(name) = assemble_lfn(&lfn_fragments) {
                    name_to_use = Some(name);
                }
            } else {
                lfn_fragments.clear();
            }
        } else {
            lfn_fragments.clear();
        }
    }

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

pub fn list_files_names<'a>(files: &'a [FileInfo]) -> Vec<&'a str> {
    files.iter().map(|f| f.name.as_str()).collect()
}

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

    pub fn ls(&self) {
        let files = list_directory_entries(&self.fs, self.current_cluster);

        print!("> ");
        for f in files.iter() {
            if f.name == "." || f.name == ".." {
                continue;
            }

            let file_type = if f.is_directory { "[DIR]" } else { "[FILE]" };

            print!("{} {} ", file_type, f.name);
        }
        print!("\n");
    }

    pub fn cd(&mut self, path: &str) -> Result<(), &str> {
        let file = self
            .fs
            .parse_path(path, Some(self.current_cluster))
            .ok_or("File not found")?;

        if !file.is_directory {
            return Err("Not a directory");
        }

        self.current_cluster = file.start_cluster;

        Ok(())
    }
}
