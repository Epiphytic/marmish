//! Fixed header and trailer witness for the bundled DELETE-journal pager.

const MAGIC: [u8; 8] = [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7];
const PAGE: u32 = 4096;
const HEADER: usize = 28;
const TRAILER: usize = 16;

pub(super) struct JournalHeader {
    pub(super) length: u64,
    pub(super) head: [u8; HEADER],
    pub(super) tail: [u8; TRAILER],
}

impl JournalHeader {
    pub(super) fn check(
        length: u64,
        main: u64,
        head: [u8; HEADER],
        tail: [u8; TRAILER],
    ) -> Result<Self, &'static str> {
        let word = |offset| u32::from_be_bytes(head[offset..offset + 4].try_into().unwrap());
        let records = word(8);
        let initial = u64::from(word(16)) * u64::from(PAGE);
        let sector = word(20);
        let complete_record = u64::from(sector) + u64::from(PAGE) + 8;
        if head[..8] != MAGIC
            || records == 0
            || initial == 0
            || initial > main
            || main > super::dispatch::CLOSED_STAGE_LIMIT
            || !main.is_multiple_of(u64::from(PAGE))
            || !(512..=65_536).contains(&sector)
            || !sector.is_power_of_two()
            || word(24) != PAGE
            || length < complete_record
        {
            return Err("journal_header");
        }
        // readSuperJournal recognizes this footer without checking pathname validity.
        if tail[..4] != [0; 4] && tail[8..] == MAGIC {
            return Err("journal_super");
        }
        Ok(Self { length, head, tail })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_header_rejects_ambiguous_journal_before_sqlite() {
        let mut head = [0; HEADER];
        head[..8].copy_from_slice(&MAGIC);
        head[8..12].copy_from_slice(&1_u32.to_be_bytes());
        head[16..20].copy_from_slice(&2_u32.to_be_bytes());
        head[20..24].copy_from_slice(&512_u32.to_be_bytes());
        head[24..].copy_from_slice(&PAGE.to_be_bytes());
        let main = u64::from(PAGE) * 2;
        let length = 512 + u64::from(PAGE) + 8;
        let tail = [0; TRAILER];

        let sentinel = JournalHeader::check(length, main, head, tail).unwrap();
        assert_eq!(sentinel.length, length);
        assert_eq!(sentinel.head, head);
        assert_eq!(sentinel.tail, tail);
        head[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(JournalHeader::check(length, main, head, tail).is_ok());

        assert!(JournalHeader::check(length - 1, main, head, tail).is_err());
        assert!(JournalHeader::check(length, main - u64::from(PAGE), head, tail).is_err());
        assert!(JournalHeader::check(length, main + 1, head, tail).is_err());
        for (offset, value) in [
            (0, 0_u32),
            (8, 0),
            (16, 0),
            (20, 768),
            (20, 131_072),
            (24, 512),
        ] {
            let mut invalid = head;
            invalid[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            assert_eq!(
                JournalHeader::check(length, main, invalid, tail).err(),
                Some("journal_header")
            );
        }

        let mut super_tail = tail;
        super_tail[8..].copy_from_slice(&MAGIC);
        assert!(JournalHeader::check(length, main, head, super_tail).is_ok());
        super_tail[3] = 1;
        assert_eq!(
            JournalHeader::check(length, main, head, super_tail).err(),
            Some("journal_super")
        );
    }
}
