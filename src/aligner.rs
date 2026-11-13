//! Safe access to edlib's alignment modes, tasks, and results.

use edlib_sys as sys;
use std::{fmt, ops::Range, os::raw::c_int, slice};

/// Which target gaps are free.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignmentMode {
    /// Align the entire query to the entire target (NW).
    #[default]
    Global,
    /// Align the query to a target prefix (SHW).
    Prefix,
    /// Align the query anywhere in the target (HW).
    Infix,
}

/// More detailed tasks require more computation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignmentTask {
    /// Compute distance and end locations.
    #[default]
    Distance,
    /// Also compute start locations.
    Locations,
    /// Also compute a path for the first pair of locations.
    Path,
}

/// A symmetric additional byte equality; equalities are not transitive.
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct EqualityPair(sys::EdlibEqualityPair);

impl EqualityPair {
    pub const fn new(first: u8, second: u8) -> Self {
        Self(sys::EdlibEqualityPair {
            first: first as _,
            second: second as _,
        })
    }
}

/// Borrowed configuration, reusable across independent alignments.
#[derive(Clone, Copy, Debug, Default)]
pub struct AlignConfig<'a> {
    pub mode: AlignmentMode,
    pub task: AlignmentTask,
    /// `None` lets edlib adaptively find the distance without an upper bound.
    pub max_distance: Option<u32>,
    pub additional_equalities: &'a [EqualityPair],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignError {
    /// Input lengths exceed a native arithmetic safety limit.
    InputTooLong,
    /// The equality count cannot be represented by a C int.
    TooManyEqualities,
    /// Edlib reported an unsuccessful status or returned an inconsistent result.
    NativeFailure,
}

impl fmt::Display for AlignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InputTooLong => "sequence lengths exceed edlib's native arithmetic limits",
            Self::TooManyEqualities => "too many additional equality pairs for edlib",
            Self::NativeFailure => "edlib alignment failed",
        })
    }
}

impl std::error::Error for AlignError {}

fn validate_lengths(query_len: usize, target_len: usize) -> Result<(), AlignError> {
    let total = query_len.checked_add(target_len);
    // buildPeq allocates (alphabetLength + 1) * queryBlocks using signed ints.
    // Reserve a row for the wildcard, even when all 256 byte values occur.
    let max_query_blocks = i32::MAX as usize / 257;
    // An empty infix can return target_len + 1 locations. Rust slices must
    // occupy at most isize::MAX bytes, including on 32-bit targets.
    if total.is_none_or(|len| len > (i32::MAX / 4) as usize)
        || query_len.div_ceil(64) > max_query_blocks
        || target_len >= isize::MAX as usize / size_of::<c_int>()
    {
        return Err(AlignError::InputTooLong);
    }
    Ok(())
}

/// Align arbitrary bytes, including NULs; inputs are not copied by this wrapper.
///
/// The result does not borrow the inputs or config.
/// Combined input length is limited to `i32::MAX / 4` to leave room for edlib's
/// signed length arithmetic, block rounding, and adaptive distance doubling.
/// Query length is additionally limited to `(i32::MAX / 257) * 64` bytes so
/// edlib's query-table element count fits a signed int for any byte alphabet.
/// Target length also leaves room for `target.len() + 1` location integers in
/// a Rust slice, which must not exceed `isize::MAX` bytes.
/// A distance limit larger than either input is clamped to their maximum length
/// (a valid upper bound on edit distance).
///
/// Empty inputs honor the distance limit and requested task. In infix mode,
/// an empty query matches every target boundary, including after the last byte.
///
/// C++ exceptions escaping edlib terminate the process.
///
/// ```
/// use edlib_rs::aligner::{align, AlignConfig, AlignmentTask, CigarFormat};
/// let config = AlignConfig { task: AlignmentTask::Path, ..Default::default() };
/// let result = align(b"ACGT", b"AGGT", &config)?;
/// assert_eq!(result.edit_distance(), Some(1));
/// assert_eq!(result.cigar(CigarFormat::Extended).unwrap().to_string(), "1=1X2=");
/// # Ok::<(), edlib_rs::aligner::AlignError>(())
/// ```
pub fn align(
    query: &[u8],
    target: &[u8],
    config: &AlignConfig<'_>,
) -> Result<Alignment, AlignError> {
    validate_lengths(query.len(), target.len())?;
    let equality_count = i32::try_from(config.additional_equalities.len())
        .map_err(|_| AlignError::TooManyEqualities)?;
    let native_config = sys::EdlibAlignConfig {
        k: config
            .max_distance
            .map(|k| k.min(query.len().max(target.len()) as u32) as i32)
            .unwrap_or(-1),
        mode: match config.mode {
            AlignmentMode::Global => sys::EdlibAlignMode_EDLIB_MODE_NW,
            AlignmentMode::Prefix => sys::EdlibAlignMode_EDLIB_MODE_SHW,
            AlignmentMode::Infix => sys::EdlibAlignMode_EDLIB_MODE_HW,
        },
        task: match config.task {
            AlignmentTask::Distance => sys::EdlibAlignTask_EDLIB_TASK_DISTANCE,
            AlignmentTask::Locations => sys::EdlibAlignTask_EDLIB_TASK_LOC,
            AlignmentTask::Path => sys::EdlibAlignTask_EDLIB_TASK_PATH,
        },
        // EqualityPair is transparent over the generated C struct.
        additionalEqualities: config.additional_equalities.as_ptr().cast(),
        additionalEqualitiesLength: equality_count,
    };
    // SAFETY: lengths fit the native arithmetic; buffers and equality pairs
    // remain live throughout this synchronous call. Edlib does not retain them.
    // The noexcept shim aborts rather than unwinding into Rust.
    let raw = unsafe {
        sys::edlibAlignNoexcept(
            query.as_ptr().cast(),
            query.len() as i32,
            target.as_ptr().cast(),
            target.len() as i32,
            native_config,
        )
    };
    // Take ownership first so the buffers are freed on every error path.
    let native = NativeResult(raw);
    check_native(&native.0, config.task, query.len())?;
    let result = Alignment {
        native,
        has_path: config.task == AlignmentTask::Path && raw.editDistance >= 0,
    };
    if !result.has_valid_locations(target.len()) {
        return Err(AlignError::NativeFailure);
    }
    Ok(result)
}

/// Rejects results whose buffers do not match what the task promises, so the
/// accessors can trust the pointers and lengths.
fn check_native(
    raw: &sys::EdlibAlignResult,
    task: AlignmentTask,
    query_len: usize,
) -> Result<(), AlignError> {
    if raw.status != sys::EDLIB_STATUS_OK as c_int {
        return Err(AlignError::NativeFailure);
    }
    let found = raw.editDistance >= 0;
    let expect = |ptr_is_null: bool, wanted: bool| ptr_is_null != (found && wanted);
    let wants_path = found && task == AlignmentTask::Path;
    let empty_path = wants_path && query_len == 0 && raw.editDistance == 0;
    // Native path-search failures leave the pointer null even on success.
    // Only a zero-distance empty query legitimately has a zero-length path.
    let valid_path = if wants_path && !empty_path {
        !raw.alignment.is_null() && raw.alignmentLength > 0
    } else {
        raw.alignment.is_null() && raw.alignmentLength == 0
    };
    let consistent = raw.editDistance >= -1
        && raw.numLocations >= 0
        && raw.alignmentLength >= 0
        && found == (raw.numLocations > 0)
        && expect(raw.endLocations.is_null(), true)
        && expect(
            raw.startLocations.is_null(),
            task != AlignmentTask::Distance,
        )
        && valid_path;
    if consistent {
        Ok(())
    } else {
        Err(AlignError::NativeFailure)
    }
}

/// Owns edlib's result buffers.
/// Accessors borrow them without copying.
#[derive(Debug)]
pub struct Alignment {
    native: NativeResult,
    has_path: bool,
}

/// Owns an edlib result, including while its contents are being validated.
#[derive(Debug)]
struct NativeResult(sys::EdlibAlignResult);

// SAFETY: the buffers are malloc allocations with no thread affinity, owned
// solely by this value, and only read through shared references.
unsafe impl Send for NativeResult {}
unsafe impl Sync for NativeResult {}

impl Drop for NativeResult {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner. Edlib initializes all allocation
        // pointers before any work, making every returned result freeable.
        unsafe { sys::edlibFreeAlignResult(self.0) }
    }
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Alignment>();
};

/// # Safety
/// `ptr` must be null or point to `len` initialized values that outlive `'a`.
unsafe fn native_slice<'a, T>(ptr: *const T, len: c_int) -> Option<&'a [T]> {
    // SAFETY: upheld by the caller; check_native guarantees len >= 0.
    (!ptr.is_null()).then(|| unsafe { slice::from_raw_parts(ptr, len as usize) })
}

impl Alignment {
    /// `None` means no alignment within the configured distance limit.
    pub fn edit_distance(&self) -> Option<u32> {
        u32::try_from(self.native.0.editDistance).ok()
    }

    /// Zero-based inclusive target end positions; `-1` denotes an empty span
    /// before the target start. Prefer [`Self::end_offsets`] or
    /// [`Self::locations`], which cannot be misread as unsigned.
    /// Empty when no alignment met the distance limit.
    pub fn end_locations(&self) -> &[i32] {
        // SAFETY: the native buffer contains numLocations initialized ints
        // and is owned by self. Validation checked the length and presence.
        unsafe {
            native_slice(self.native.0.endLocations, self.native.0.numLocations).unwrap_or(&[])
        }
    }

    /// Zero-based inclusive starts corresponding to [`Self::end_locations`],
    /// or `None` if not computed. A start exceeds its end when the span is empty.
    pub fn start_locations(&self) -> Option<&[i32]> {
        // SAFETY: the native buffer contains numLocations initialized ints
        // when present and is owned by self.
        unsafe { native_slice(self.native.0.startLocations, self.native.0.numLocations) }
    }

    /// Exclusive target end offsets, in the order of [`Self::end_locations`].
    pub fn end_offsets(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        // has_valid_locations guarantees end >= -1.
        self.end_locations().iter().map(|&end| (end + 1) as usize)
    }

    /// Half-open target spans of the optimal alignments, in the order of
    /// [`Self::end_locations`], or `None` if start locations were not computed.
    /// The path covers the first span.
    pub fn locations(&self) -> Option<impl ExactSizeIterator<Item = Range<usize>> + '_> {
        let starts = self.start_locations()?;
        // has_valid_locations guarantees 0 <= start <= end + 1.
        Some(
            starts
                .iter()
                .zip(self.end_offsets())
                .map(|(&start, end)| start as usize..end),
        )
    }

    /// Path for the first location pair, or `None` if not computed.
    /// A computed zero-length path is `Some(&[])`, not `None`.
    pub fn path(&self) -> Option<&[EditOperation]> {
        if !self.has_path {
            return None;
        }
        // SAFETY: edlib writes only codes 0..=3, matching this repr(u8) enum.
        // The buffer has alignmentLength initialized bytes and is owned by
        // self. Validation permits a null pointer only for an empty path.
        Some(unsafe {
            native_slice(self.native.0.alignment.cast(), self.native.0.alignmentLength)
                .unwrap_or(&[])
        })
    }

    /// A borrowed, allocation-free CIGAR formatter, or `None` without a path.
    pub fn cigar(&self, format: CigarFormat) -> Option<Cigar<'_>> {
        self.path().map(|path| Cigar { path, format })
    }

    fn has_valid_locations(&self, target_len: usize) -> bool {
        let ends = self.end_locations();
        let target_len = target_len as i32;
        ends.iter().all(|&end| (-1..target_len).contains(&end))
            && self.start_locations().is_none_or(|starts| {
                starts
                    .iter()
                    .zip(ends)
                    .all(|(&start, &end)| (0..=end + 1).contains(&start))
            })
    }
}

/// Edlib path codes. With query = read and target = reference, I/D follow SAM.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOperation {
    Match = 0,
    /// Consumes a query byte only (insertion into the target).
    Insert = 1,
    /// Consumes a target byte only (deletion from the target).
    Delete = 2,
    Mismatch = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CigarFormat {
    /// Matches and mismatches are both `M`.
    Standard,
    /// Matches are `=` and mismatches are `X`.
    Extended,
}

/// Formats a path as run-length encoded CIGAR without an intermediate buffer.
/// Use `to_string()` when an owned string is needed.
#[derive(Clone, Copy, Debug)]
pub struct Cigar<'a> {
    path: &'a [EditOperation],
    format: CigarFormat,
}

impl fmt::Display for Cigar<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut codes = self.path.iter().map(|op| match (op, self.format) {
            (EditOperation::Insert, _) => 'I',
            (EditOperation::Delete, _) => 'D',
            (EditOperation::Match, CigarFormat::Extended) => '=',
            (EditOperation::Mismatch, CigarFormat::Extended) => 'X',
            _ => 'M',
        });
        let Some(mut previous) = codes.next() else {
            return Ok(());
        };
        let mut count = 1;
        for code in codes {
            if code == previous {
                count += 1;
            } else {
                write!(f, "{count}{previous}")?;
                previous = code;
                count = 1;
            }
        }
        write!(f, "{count}{previous}")
    }
}

#[cfg(test)]
mod tests {
    use super::{AlignError, AlignmentTask, check_native, sys, validate_lengths};
    use std::ptr;

    #[test]
    fn length_validation_prevents_native_table_overflow() {
        // This fits the combined-length limit but overflows buildPeq's int
        // multiplication with a 256-byte alphabet plus its wildcard row.
        assert_eq!(
            validate_lengths(536_870_848, 1),
            Err(AlignError::InputTooLong)
        );
        let largest_query = (i32::MAX as usize / 257) * 64;
        assert_eq!(validate_lengths(largest_query, 1), Ok(()));
        assert_eq!(
            validate_lengths(largest_query + 1, 1),
            Err(AlignError::InputTooLong)
        );

        let largest_total = (i32::MAX / 4) as usize;
        let largest_target =
            largest_total.min(isize::MAX as usize / size_of::<std::os::raw::c_int>() - 1);
        assert_eq!(validate_lengths(0, largest_target), Ok(()));
        assert_eq!(
            validate_lengths(0, largest_target + 1),
            Err(AlignError::InputTooLong)
        );
        assert_eq!(
            validate_lengths(usize::MAX, 1),
            Err(AlignError::InputTooLong)
        );
    }

    #[test]
    fn native_results_missing_requested_buffers_are_failures() {
        let mut locations = [3];
        let mut path = [0u8];
        let found = sys::EdlibAlignResult {
            status: sys::EDLIB_STATUS_OK as _,
            editDistance: 0,
            endLocations: locations.as_mut_ptr(),
            startLocations: locations.as_mut_ptr(),
            numLocations: 1,
            alignment: path.as_mut_ptr(),
            alignmentLength: 1,
            alphabetLength: 1,
        };
        assert_eq!(check_native(&found, AlignmentTask::Path, 1), Ok(()));
        let missed = sys::EdlibAlignResult {
            editDistance: -1,
            endLocations: ptr::null_mut(),
            startLocations: ptr::null_mut(),
            numLocations: 0,
            alignment: ptr::null_mut(),
            alignmentLength: 0,
            ..found
        };
        assert_eq!(check_native(&missed, AlignmentTask::Path, 1), Ok(()));

        // Edlib discards path-search failures but still reports success.
        let pathless = sys::EdlibAlignResult {
            alignment: ptr::null_mut(),
            alignmentLength: 0,
            ..found
        };
        assert_eq!(
            check_native(&pathless, AlignmentTask::Path, 1),
            Err(AlignError::NativeFailure)
        );
        let startless = sys::EdlibAlignResult {
            startLocations: ptr::null_mut(),
            ..found
        };
        assert_eq!(
            check_native(&startless, AlignmentTask::Locations, 1),
            Err(AlignError::NativeFailure)
        );
        let errored = sys::EdlibAlignResult {
            status: sys::EDLIB_STATUS_ERROR as _,
            ..found
        };
        assert_eq!(
            check_native(&errored, AlignmentTask::Path, 1),
            Err(AlignError::NativeFailure)
        );

        let mut empty_end = [-1];
        let mut empty_start = [0];
        let empty_path = sys::EdlibAlignResult {
            endLocations: empty_end.as_mut_ptr(),
            startLocations: empty_start.as_mut_ptr(),
            ..pathless
        };
        assert_eq!(check_native(&empty_path, AlignmentTask::Path, 0), Ok(()));
        assert_eq!(
            check_native(&empty_path, AlignmentTask::Path, 1),
            Err(AlignError::NativeFailure)
        );
    }
}
