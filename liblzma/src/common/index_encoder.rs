use crate::api::LzmaIndexIter;

use super::LzmaIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sequence {
    #[default]
    SeqIndicator,
    SeqCount,
    SeqUnpadded,
    SeqUncompressed,
    SeqNext,
    SeqPadding,
    SeqCrc32,
}
#[derive(Default, Debug)]
pub struct LzmaIndexEncoder {
    /// The sequence type
    sequence: Sequence,

    /// Index being encoded
    index: Option<Box<LzmaIndex>>,

    /// Iterator for the Index being encoded
    iter: LzmaIndexIter,

    /// Position in integers
    pos: usize,

    /// CRC32 of the List of Records field
    crc32: u32,
}

impl Clone for LzmaIndexEncoder {
    fn clone(&self) -> Self {
        Self {
            sequence: self.sequence.clone(),
            index: self.index.clone(),
            iter: self.iter.clone(),
            pos: self.pos,
            crc32: self.crc32,
        }
    }
}
