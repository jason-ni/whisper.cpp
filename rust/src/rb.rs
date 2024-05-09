use std::cmp;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use crate::accel::convert_pcm16_to_f32;

/// Managment interface for the ring buffer.
pub trait RB {
    /// Resets the whole buffer to the default value of type `T`.
    /// The buffer is empty after this call.
    fn clear(&self);
    /// Creates a *producer* view inside the buffer.
    fn producer(&self) -> Producer;
    /// Creates a *consumer* view inside the buffer.
    fn consumer(&self) -> Consumer;
}

/// RbInspector provides non-modifying operations on the ring buffer.
pub trait RbInspector {
    /// Returns true if the buffer is empty.
    fn is_empty(&self) -> bool;
    /// Returns true if the buffer is full.
    fn is_full(&self) -> bool;
    /// Returns the total capacity of the ring buffer.
    /// This is the size with which the buffer was initialized.
    fn capacity(&self) -> usize;
    /// Returns the number of values that can be written until the buffer until it is full.
    fn slots_free(&self) -> usize;
    /// Returns the number of values from the buffer that are available to read.
    fn count(&self) -> usize;
    /// Returns whether the ring buffer is closed
    fn is_closed(&self) -> bool;
    fn close(&self);
}

/// Defines *write* methods for a producer view.
pub trait RbProducer {
    /// Works analog to `write` but blocks until there are free slots in the ring buffer.
    /// The number of actual blocks written is returned in the `Option` value.
    ///
    /// Returns `None` if the given slice has zero length.
    fn write_blocking(&self, data: &[i16]) -> Result<Option<usize>>;
    /// Works analog to `write_blocking` but eventually returns if the specified timeout is reached.
    /// The number of actual blocks written is returned in the `Ok(Option)` value.
    ///
    /// Returns `Ok(None)` if the given slice has zero length.
    ///
    /// Possible errors:
    ///
    /// - `RbError::TimedOut`
    fn write_blocking_timeout(&self, data: &[i16], timeout: Duration) -> Result<Option<usize>>;
    fn write_ext_blocking(&self, data: &[i16]) -> Result<()>;
    fn close(&self);
}

/// Defines *read* methods for a consumer view.
pub trait RbConsumer {
    fn peek_ext(&self, pos: usize, data: &mut [f32]) -> Result<SampleRange>;
    fn peek_blocking(&self, pos: usize, data: &mut [f32]) -> Result<SampleRange>;
    fn peek_time_range(&self, start: usize, end: usize, data: &mut [f32]) -> Result<SampleRange>;
    fn commit_read(&self, cnt: usize);
}

/// Ring buffer errors.
#[derive(Debug, thiserror::Error)]
pub enum RbError {
    Full,
    Empty,
    TimedOut,
    Again,
    EOF(SampleRange),
}
impl fmt::Display for RbError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RbError::Full => write!(f, "No free slots in the buffer"),
            RbError::Empty => write!(f, "Buffer is empty"),
            RbError::TimedOut => write!(f, "Timed out waiting for available slots"),
            RbError::Again => write!(f, "Try again"),
            RbError::EOF(_) => write!(f, "End of data"),
        }
    }
}

/// Result type used inside the module.
pub type Result<T> = std::result::Result<T, RbError>;

#[derive(Debug)]
pub enum SampleRange {
    Adjacent(* const f32, usize),
    NonAdjacent(usize),
    EofEmpty,
}

struct Inspector {
    pub gpos: Arc<AtomicUsize>,
    read_pos: Arc<AtomicUsize>,
    write_pos: Arc<AtomicUsize>,
    size: usize,
    closed: Arc<AtomicBool>,
}

impl Inspector {
    #[allow(dead_code)]
    fn show_state(&self, owner: &str) {
        println!("[{}]: read_pos: {}, write_pos: {}, slots_free: {}, count: {}",
                 owner,
                 self.read_pos.load(Ordering::Relaxed),
                 self.write_pos.load(Ordering::Relaxed),
                 self.slots_free(),
                 self.count());
    }
}

/// A *thread-safe* Single-Producer-Single-Consumer RingBuffer
///
/// - blocking and non-blocking IO
/// - mutually exclusive access for producer and consumer
/// - no use of `unsafe`
/// - never under- or overflows
///
/// ```
/// use std::thread;
/// use rb::*;
///
/// let rb = SpscRb::new(1024);
/// let (prod, cons) = (rb.producer(), rb.consumer());
/// thread::spawn(move || {
///     let gen = || {(-16..16+1).cycle().map(|x| x as f32/16.0)};
///     loop {
///         let data = gen().take(32).collect::<Vec<f32>>();
///         prod.write(&data).unwrap();
///     }
/// });
/// let mut data = Vec::with_capacity(1024);
/// let mut buf = [0.0f32; 256];
/// while data.len() < 1024 {
///     let cnt = cons.read_blocking(&mut buf).unwrap();
///     data.extend_from_slice(&buf[..cnt]);
/// }
/// ```
pub struct SpscRb {
    buf: Arc<Mutex<Vec<f32>>>,
    inspector: Arc<Inspector>,
    slots_free: Arc<Condvar>,
    data_available: Arc<Condvar>,
}

impl SpscRb {
    #[allow(dead_code)]
    pub fn new(size: usize) -> Self {
        let (read_pos, write_pos) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        SpscRb {
            buf: Arc::new(Mutex::new(vec![f32::default(); size + 1])),
            slots_free: Arc::new(Condvar::new()),
            data_available: Arc::new(Condvar::new()),
            // the additional element is used to distinct between empty and full state
            inspector: Arc::new(Inspector {
                gpos: Arc::new(AtomicUsize::new(0)),
                read_pos,
                write_pos,
                size: size + 1,
                closed: Arc::new(AtomicBool::new(false)),
            }),
        }
    }
}

impl RB for SpscRb {
    fn clear(&self) {
        let mut buf = self.buf.lock().unwrap();
        buf.iter_mut().map(|_| f32::default()).count();
        self.inspector.read_pos.store(0, Ordering::Relaxed);
        self.inspector.write_pos.store(0, Ordering::Relaxed);
    }

    fn producer(&self) -> Producer {
        Producer {
            buf: self.buf.clone(),
            inspector: self.inspector.clone(),
            slots_free: self.slots_free.clone(),
            data_available: self.data_available.clone(),
        }
    }

    fn consumer(&self) -> Consumer {
        Consumer {
            buf: self.buf.clone(),
            inspector: self.inspector.clone(),
            slots_free: self.slots_free.clone(),
            data_available: self.data_available.clone(),
        }
    }
}

impl RbInspector for SpscRb {
    fn is_empty(&self) -> bool {
        self.inspector.is_empty()
    }
    fn is_full(&self) -> bool {
        self.inspector.is_full()
    }
    fn capacity(&self) -> usize {
        self.inspector.capacity()
    }
    fn slots_free(&self) -> usize {
        self.inspector.slots_free()
    }
    fn count(&self) -> usize {
        self.inspector.count()
    }
    fn is_closed(&self) -> bool {
        self.inspector.is_closed()
    }
    fn close(&self) {
        self.inspector.close();
        self.data_available.notify_one();
    }
}

impl RbInspector for Inspector {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.slots_free() == self.capacity()
    }

    #[inline(always)]
    fn is_full(&self) -> bool {
        self.slots_free() == 0
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        self.size - 1
    }

    #[inline(always)]
    fn slots_free(&self) -> usize {
        let wr_pos = self.write_pos.load(Ordering::Relaxed);
        let re_pos = self.read_pos.load(Ordering::Relaxed);
        if wr_pos < re_pos {
            re_pos - wr_pos - 1
        } else {
            self.capacity() - wr_pos + re_pos
        }
    }

    #[inline(always)]
    fn count(&self) -> usize {
        self.capacity() - self.slots_free()
    }

    #[inline(always)]
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    #[inline(always)]
    fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }
}

/// Producer view into the ring buffer.
pub struct Producer {
    buf: Arc<Mutex<Vec<f32>>>,
    inspector: Arc<Inspector>,
    slots_free: Arc<Condvar>,
    data_available: Arc<Condvar>,
}

impl Producer {
    #[allow(dead_code)]
    pub fn show_state(&self) {
        self.inspector.show_state("producer");
    }
}

/// Consumer view into the ring buffer.
pub struct Consumer {
    buf: Arc<Mutex<Vec<f32>>>,
    inspector: Arc<Inspector>,
    slots_free: Arc<Condvar>,
    data_available: Arc<Condvar>,
}

impl Consumer {
    #[allow(dead_code)]
    pub fn show_state(&self) {
        self.inspector.show_state("consumer");
    }
}

impl RbProducer for Producer {
    fn write_blocking(&self, data: &[i16]) -> Result<Option<usize>> {
        //println!("write_blocking: data.len() = {}", data.len());
        let ret = self.write_blocking_timeout(data, Duration::MAX);
        //self.show_state();
        ret
    }

    fn write_blocking_timeout(&self, data: &[i16], timeout: Duration) -> Result<Option<usize>> {
        if data.is_empty() {
            return Ok(None);
        }

        let guard = self.buf.lock().unwrap();
        if self.inspector.is_closed() {
            return Err(RbError::EOF(SampleRange::EofEmpty));
        }
        let mut buf = if self.inspector.is_full() {
            if timeout == Duration::MAX {
                // No need to call wait_timeout if the duration is max
                self.slots_free.wait(guard).unwrap()
            } else {
                let (guard, result) = self.slots_free.wait_timeout(guard, timeout).unwrap();
                if result.timed_out() {
                    return Err(RbError::TimedOut);
                }
                guard
            }
        } else {
            guard
        };

        let buf_len = buf.len();
        let data_len = data.len();
        let wr_pos = self.inspector.write_pos.load(Ordering::Relaxed);
        let cnt = cmp::min(data_len, self.inspector.slots_free());

        if (wr_pos + cnt) < buf_len {
            convert_pcm16_to_f32(&data[..cnt], &mut buf[wr_pos..wr_pos + cnt]);
        } else {
            let d = buf_len - wr_pos;
            convert_pcm16_to_f32(&data[..d], &mut buf[wr_pos..]);
            convert_pcm16_to_f32(&data[d..cnt], &mut buf[..(cnt-d)]);
        }
        self.inspector
            .write_pos
            .store((wr_pos + cnt) % buf_len, Ordering::Relaxed);

        self.data_available.notify_one();
        Ok(Some(cnt))
    }

    fn write_ext_blocking(&self, data: &[i16]) -> Result<()> {
        let buf_len = data.len();
        let mut pos = 0usize;
        while let Some(written) = self.write_blocking(&data[pos..])? {
            pos += written;
            if pos == buf_len {
                break;
            }
        }
        Ok(())
    }

    fn close(&self) {
        self.inspector.close();
        self.data_available.notify_one();
    }
}

impl RbConsumer for Consumer {

    fn peek_ext(&self, pos: usize, data: &mut [f32]) -> Result<SampleRange> {
        let guard = self.buf.lock().unwrap();
        let gpos = self.inspector.gpos.load(Ordering::Relaxed);
        if gpos > pos {
            panic!("can't read data already read committed")
        }
        let re_pos_offset = pos - gpos;
        let available_cnt = self.inspector.count() - re_pos_offset;
        let mut req_cnt = data.len();
        let mut is_tail_partial = false;
        if available_cnt < req_cnt {
            if self.inspector.is_closed() {
                is_tail_partial = true;
                if available_cnt == 0 {
                    return Err(RbError::EOF(SampleRange::EofEmpty));
                }
                req_cnt = available_cnt;
            } else {
                return Err(RbError::Again)
            }
        }
        let buf = guard.as_slice();
        let buf_len = buf.len();
        let re_pos = (self.inspector.read_pos.load(Ordering::Relaxed) + re_pos_offset) % buf_len;
        if (re_pos + req_cnt) < buf_len {
            //println!("peek_f32_ext: req_cnt = {}, available_cnt = {}", req_cnt, available_cnt);
            //self.show_state();
            //data[..req_cnt].copy_from_slice(&buf[re_pos..re_pos + req_cnt]);
            // if sample range is adjacent to buffer, return a pointer to the buffer
            if !is_tail_partial {
                Ok(SampleRange::Adjacent(buf[re_pos..re_pos + req_cnt].as_ptr(), req_cnt))
            } else {
                Err(RbError::EOF(SampleRange::Adjacent(buf[re_pos..re_pos + req_cnt].as_ptr(), req_cnt)))
            }
        } else {
            let d = buf_len - re_pos;
            //println!("peek_f32_ext: req_cnt = {}, d = {}", req_cnt, d);
            data[..d].copy_from_slice(&buf[re_pos..]);
            data[d..].copy_from_slice(&buf[..(req_cnt - d)]);
            if !is_tail_partial {
                Ok(SampleRange::NonAdjacent(req_cnt))
            } else {
                Err(RbError::EOF(SampleRange::NonAdjacent(req_cnt)))
            }
        }
    }

    fn peek_blocking(&self, pos: usize, data: &mut [f32]) -> Result<SampleRange> {
        loop {
            //println!("pos: Peeking at position {}, data.len() = {}", pos, data.len());
            //self.show_state();
            match self.peek_ext(pos, data) {
                Ok(sr) => return Ok(sr),
                Err(RbError::Again) => continue,
                Err(RbError::EOF(sr)) => return Err(RbError::EOF(sr)),
                _ => (),
            }
            let guard = self.buf.lock().unwrap();
            let _buf = self.data_available.wait(guard).unwrap();
        }
    }

    fn peek_time_range(&self, start: usize, end: usize, data: &mut [f32]) -> Result<SampleRange> {
        let start_pos = start * 16;
        let end_pos = end * 16;
        let guard = self.buf.lock().unwrap();
        let gpos = self.inspector.gpos.load(Ordering::Relaxed);
        if gpos > start_pos {
            panic!("peek_time_range: can't read data already read committed")
        }
        let req_cnt = end_pos - start_pos;
        let available_cnt = self.inspector.count() - (start_pos - gpos);
        if available_cnt < req_cnt {
            panic!("peek_time_range: can't read data, not enough")
        }
        let re_pos = self.inspector.read_pos.load(Ordering::Relaxed);
        let wr_pos = self.inspector.write_pos.load(Ordering::Relaxed);
        let buf = guard.as_slice();
        let buf_len = buf.len();
        let read_start = (re_pos + (start_pos - gpos)) % buf_len;
        if (read_start + req_cnt) < buf_len {
            Ok(SampleRange::Adjacent(buf[read_start..read_start + req_cnt].as_ptr(), req_cnt))
        } else {
            let d = buf_len - read_start;
            data[..d].copy_from_slice(&buf[read_start..]);
            data[d..req_cnt].copy_from_slice(&buf[..(req_cnt - d)]);
            Ok(SampleRange::NonAdjacent(req_cnt))
        }
    }

    fn commit_read(&self, read_end: usize) {
        let guard = self.buf.lock().unwrap();
        let buf_len = guard.as_slice().len();
        let gpos = self.inspector.gpos.load(Ordering::Relaxed);
        let cnt = read_end - gpos;
        let available_cnt = self.inspector.count();
        if available_cnt < cnt {
            panic!("can't commit data, not enough")
        }
        self.inspector.gpos.store(read_end, Ordering::Relaxed);
        let re_pos = self.inspector.read_pos.load(Ordering::Relaxed);
        self.inspector.read_pos.store((re_pos + cnt) % buf_len, Ordering::Relaxed);
        self.slots_free.notify_one();
    }

}
