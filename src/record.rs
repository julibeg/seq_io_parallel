use std::borrow::Cow;

pub trait MinimalRefRecord<'a> {
    fn ref_head(&self) -> &[u8];

    fn ref_seq(&self) -> &[u8];

    fn ref_full_seq(&self) -> Cow<[u8]>;

    fn ref_seq_lines(&self) -> impl Iterator<Item = &'_[u8]>;

    fn ref_qual(&self) -> &[u8];
}

impl MinimalRefRecord<'_> for seq_io::fastq::RefRecord<'_> {
    fn ref_head(&self) -> &[u8] {
        <Self as seq_io::fastq::Record>::head(self)
    }

    fn ref_seq(&self) -> &[u8] {
        <Self as seq_io::fastq::Record>::seq(self)
    }

    fn ref_full_seq(&self) -> Cow<[u8]> {
        Cow::Borrowed(self.ref_seq())
    }

    fn ref_seq_lines(&self) -> impl Iterator<Item = &'_[u8]> {
        // simply return an iterator yielding the full sequence
        std::iter::once(self.ref_seq())
    }

    fn ref_qual(&self) -> &[u8] {
        <Self as seq_io::fastq::Record>::qual(self)
    }
}

impl MinimalRefRecord<'_> for seq_io::fasta::RefRecord<'_> {
    fn ref_head(&self) -> &[u8] {
        <Self as seq_io::fasta::Record>::head(self)
    }

    fn ref_seq(&self) -> &[u8] {
        <Self as seq_io::fasta::Record>::seq(self)
    }

    fn ref_full_seq(&self) -> Cow<[u8]> {
        self.full_seq()
    }

    fn ref_seq_lines(&self) -> impl Iterator<Item = &'_[u8]> {
        Box::new(self.seq_lines())
    }

    fn ref_qual(&self) -> &[u8] {
        &[]
    }
}
