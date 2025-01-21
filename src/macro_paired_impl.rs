use std::{io, sync::Arc, thread};

use anyhow::Result;
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use seq_io::policy;

use crate::{PairedParallelProcessor, PairedParallelReader};

/// Type alias for synchronized record sets containing pairs of records
type PairedRecordSets<T> = Arc<Vec<Mutex<(T, T)>>>;
/// Type alias for channels used in parallel processing
type ProcessorChannels = (Sender<Option<usize>>, Receiver<Option<usize>>);

/// Creates a collection of paired record sets for parallel processing
///
/// Note: The number of record sets is twice the number of threads
/// to allow for double buffering. Each set contains two records (R1 and R2)
fn create_paired_record_sets<T: Default>(num_threads: usize) -> PairedRecordSets<T> {
    let record_sets = (0..num_threads * 2)
        .map(|_| Mutex::new((T::default(), T::default())))
        .collect();
    Arc::new(record_sets)
}

/// Creates a pair of channels for communication between reader and worker threads
fn create_channels(buffer_size: usize) -> ProcessorChannels {
    bounded(buffer_size)
}

/// Internal processing of reader thread for paired reads
fn run_paired_reader_thread<R, T, F>(
    mut reader1: R,
    mut reader2: R,
    record_sets: PairedRecordSets<T>,
    tx: Sender<Option<usize>>,
    num_threads: usize,
    read_fn: F,
) -> Result<()>
where
    F: Fn(&mut R, &mut T) -> Option<Result<()>>,
{
    let mut current_idx = 0;

    loop {
        let mut record_set_pair = record_sets[current_idx].lock();

        match (
            read_fn(&mut reader1, &mut record_set_pair.0),
            read_fn(&mut reader2, &mut record_set_pair.1),
        ) {
            (Some(result1), Some(result2)) => {
                // Process both results
                result1?;
                result2?;

                drop(record_set_pair);
                tx.send(Some(current_idx)).unwrap();
                current_idx = (current_idx + 1) % record_sets.len();
            }
            _ => break, // EOF on either file
        }
    }

    // Signal completion to all workers
    for _ in 0..num_threads {
        tx.send(None).unwrap();
    }

    Ok(())
}

/// Internal processing of worker threads for paired reads
fn run_paired_worker_thread<T, P, F>(
    record_sets: PairedRecordSets<T>,
    rx: Receiver<Option<usize>>,
    mut processor: P,
    process_fn: F,
) -> Result<()>
where
    P: PairedParallelProcessor,
    F: Fn(&(T, T), &mut P) -> Result<()>,
{
    while let Ok(Some(idx)) = rx.recv() {
        let record_set_pair = record_sets[idx].lock();
        process_fn(&record_set_pair, &mut processor)?;
        processor.on_batch_complete()?;
    }
    Ok(())
}

/// Macro to implement parallel reader for paired reads
macro_rules! impl_paired_parallel_reader {
    ($reader:ty, $record_set:ty, $error:ty) => {
        impl<R, P> PairedParallelReader<R, P> for $reader
        where
            R: io::Read + Send,
            P: policy::BufPolicy + Send,
        {
            fn process_parallel_paired<T>(
                self,
                reader2: Self,
                processor: T,
                num_threads: usize,
            ) -> Result<()>
            where
                T: PairedParallelProcessor,
            {
                let record_sets = create_paired_record_sets::<$record_set>(num_threads);
                let (tx, rx) = create_channels(num_threads * 2);

                thread::scope(|scope| -> Result<()> {
                    // Spawn reader thread
                    let reader_sets = Arc::clone(&record_sets);
                    let reader_handle = scope.spawn(move || -> Result<()> {
                        run_paired_reader_thread(
                            self,
                            reader2,
                            reader_sets,
                            tx,
                            num_threads,
                            |reader, record_set| {
                                reader
                                    .read_record_set(record_set)
                                    .map(|result| result.map_err(Into::into))
                            },
                        )
                    });

                    // Spawn worker threads
                    let mut handles = Vec::new();
                    for _ in 0..num_threads {
                        let worker_sets = Arc::clone(&record_sets);
                        let worker_rx = rx.clone();
                        let worker_processor = processor.clone();

                        let handle = scope.spawn(move || {
                            run_paired_worker_thread(
                                worker_sets,
                                worker_rx,
                                worker_processor,
                                |record_set_pair, processor| {
                                    let records1 = record_set_pair.0.into_iter();
                                    let records2 = record_set_pair.1.into_iter();

                                    // Process pairs of records
                                    for (r1, r2) in records1.zip(records2) {
                                        processor.process_record_pair(r1, r2)?;
                                    }
                                    Ok(())
                                },
                            )
                        });

                        handles.push(handle);
                    }

                    // Wait for reader thread
                    reader_handle.join().unwrap()?;

                    // Wait for worker threads
                    for handle in handles {
                        handle.join().unwrap()?;
                    }

                    Ok(())
                })?;

                Ok(())
            }
        }
    };
}

// Use the macro to implement for both FASTA and FASTQ
impl_paired_parallel_reader!(seq_io::fasta::Reader<R, P>, seq_io::fasta::RecordSet, seq_io::fasta::Error);
impl_paired_parallel_reader!(seq_io::fastq::Reader<R, P>, seq_io::fastq::RecordSet, seq_io::fastq::Error);
