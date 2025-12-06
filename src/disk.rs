use alloc::vec::Vec;

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
    pub fn new(disk: &'static [u8]) -> Self {
        const BPS_OFFSET: usize = BootOffsets::BytsPerSec as usize;
        let bytes_per_sector = u16::from_le_bytes(
            disk[BPS_OFFSET..BPS_OFFSET + 2]
                .try_into()
                .expect("Error reading bytes per sector"),
        ) as u32;

        const SPC_OFFSET: usize = BootOffsets::SecPerClus as usize;
        let sectors_per_cluster = disk[SPC_OFFSET] as u32;

        const RSC_OFFSET: usize = BootOffsets::RsvdSecCnt as usize;
        let reserved_sectors_count = u16::from_le_bytes(
            disk[RSC_OFFSET..RSC_OFFSET + 2]
                .try_into()
                .expect("Error readinf from reserved clustors count"),
        ) as u32;

        const NF_OFFSET: usize = BootOffsets::NumFATs as usize;
        let num_fats = disk[NF_OFFSET] as u32;

        const SPF_OFFSET: usize = BootOffsets::FATSz32 as usize;
        let sectors_per_fat = u32::from_le_bytes(
            disk[SPF_OFFSET..SPF_OFFSET + 4]
                .try_into()
                .expect("Error reading occupied sectors per fat"),
        );

        const RC_OFFSET: usize = BootOffsets::RootClus as usize;
        let root_cluster = u32::from_le_bytes(
            disk[RC_OFFSET..RC_OFFSET + 4]
                .try_into()
                .expect("Error reading root cluster"),
        );

        let fat_sector = reserved_sectors_count;
        let data_sector = reserved_sectors_count + (num_fats * sectors_per_fat);

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

    fn parse_cluster(data: &[u8]) -> FatDir {
        const N_OFFSET: usize = DirOffsets::Name as usize;
        let name: [u8; 11] = data[N_OFFSET..N_OFFSET + 11]
            .try_into()
            .expect("Can't read dir name");

        const ATT_OFFSET: usize = DirOffsets::Attr as usize;
        let attr = data[ATT_OFFSET];

        const FCH_OFFSET: usize = DirOffsets::FstClusHI as usize;
        let first_cluster_high = u16::from_le_bytes(
            data[FCH_OFFSET..FCH_OFFSET + 2]
                .try_into()
                .expect("Can't read first cluster high"),
        );

        const FCL_OFFSET: usize = DirOffsets::FstClusLO as usize;
        let first_cluster_low = u16::from_le_bytes(
            data[FCL_OFFSET..FCL_OFFSET + 2]
                .try_into()
                .expect("Can't read first cluster low"),
        );

        const FS_OFFSET: usize = DirOffsets::FileSize as usize;
        let size = u32::from_le_bytes(
            data[FS_OFFSET..FS_OFFSET + 4]
                .try_into()
                .expect("Can't read size"),
        );

        FatDir {
            name,
            attr,
            first_cluster_high,
            first_cluster_low,
            size,
        }
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
    Attr = 13,
    FstClusHI = 20,
    FstClusLO = 26,
    FileSize = 28,
}
