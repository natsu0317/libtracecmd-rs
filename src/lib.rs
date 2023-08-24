// Copyright 2023 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![deny(missing_docs, rustdoc::broken_intra_doc_links)]

//! This library is a Rust wrapper of
//! [libtracecmd](https://www.trace-cmd.org/Documentation/libtracecmd/), which allows writing
//! programs to analyze Linux's [ftrace](https://docs.kernel.org/trace/ftrace.html) data
//! generated by [trace-cmd](https://github.com/rostedt/trace-cmd).
//!
//! # Running a Sample Program
//!
//! To get familiar with using this library, you can start by running [a sample program](https://github.com/google/libtracecmd-rs/blob/main/examples/top_n_events.rs).
//!
//! ## Preliminary
//!
//! First, make sure that `CONFIG_FTRACE` and `CONFIG_FTRACE_SYSCALLS` are enabled in your Linux
//! kernel.
//! Then,  install a `trace-cmd` binary and libraries to analyze trace data files. If you use
//! Debian or Ubuntu, they should be installed with the following command:
//!
//! ```bash
//! $ sudo apt install \
//!     trace-cmd \
//!     libtracefs-dev \
//!     libtraceevent-dev \
//!     libtracecmd-dev
//! ```
//!
//! ## Get tracing record
//!
//! Run `trace-cmd record` along with your own workloads to record trace events.
//!
//! ```bash
//! # Trace all syscalls called on your system during 10 seconds.
//! $ trace-cmd record -e syscalls sleep 10
//! # Then, you can run your own workload to be traced.
//! ```
//!
//! Then, you'll find `trace.dat` in the current directory.
//!
//! ## Analyze `trace.dat` with a sample program
//!
//! Now, you can run
//! [a sample code `top_n_events`](https://github.com/google/libtracecmd-rs/blob/main/examples/top_n_events.rs)
//! to analyze the `trace.dat`.
//!
//! ```bash
//! $ git clone git@github.com:google/libtracecmd-rs.git
//! $ cd ./libtracecmd-rs
//! $ cargo run --example top_n_events -- --input ./trace.dat --n 10 --prefix sys_enter_
//! ```
//!
//! Then, you'll get output like the followings:
//! ```text
//! Top 10 events:
//! #1: ioctl: 62424 times
//! #2: futex: 59074 times
//! #3: read: 30144 times
//! #4: write: 28361 times
//! #5: newfstatat: 22590 times
//! #6: close: 15893 times
//! #7: splice: 14650 times
//! #8: getuid: 13579 times
//! #9: epoll_pwait: 12298 times
//! #10: ppoll: 10523 times
//! ```
//!
//! # Writing your own code with the library
//!
//! See the documenation on [Handler].

#[allow(
    clippy::upper_case_acronyms,
    clippy::useless_transmute,
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code
)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

use thiserror::Error;

/// Errors that can happen while processing tracing data.
#[derive(Error, Debug)]
pub enum Error {
    /// Failed to open .dat file
    #[error("failed to open .dat file")]
    Open,
    /// Failed to get `tep_handle`
    #[error("failed to get tep_handle")]
    Handle,
    /// Failed to find `tep_handle`
    #[error("failed to find tep_event")]
    FindEvent,
    /// Failed to find `tep_field`
    #[error("failed to find tep_field")]
    FindField,
    /// Invalid PID
    #[error("invalid PID: {0}")]
    InvalidPid(String),
    /// Invalid timestamp
    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),
    /// Invalid string
    #[error("invalid string: {0}")]
    InvalidString(std::str::Utf8Error),
    /// Failed to read a field
    #[error("failed to read a field")]
    ReadField,
}

type Result<T> = std::result::Result<T, Error>;

unsafe fn cptr_to_string(ptr: *mut i8) -> Result<String> {
    let c_str: &std::ffi::CStr = unsafe { std::ffi::CStr::from_ptr(ptr) };
    Ok(c_str.to_str().map_err(Error::InvalidString)?.to_string())
}

/// A wrapper of `tracecmd_input` represnting a `trace.dat` file given as the input.
pub struct Input(*mut bindings::tracecmd_input);

impl Input {
    /// Opens a given `trace.dat` file and create `Input`.
    pub fn new(path: &str) -> Result<Self> {
        // TODO: Support open flags.
        let handle = unsafe { bindings::tracecmd_open(path.as_ptr() as *mut i8, 0) };
        if handle.is_null() {
            return Err(Error::Open);
        }

        Ok(Input(handle))
    }

    /// Gets `Handle` from the `Input`.
    pub fn handle_ref(&self) -> Result<HandleRef> {
        let ret = unsafe { bindings::tracecmd_get_tep(self.0) };
        if ret.is_null() {
            Err(Error::Handle)
        } else {
            Ok(HandleRef(ret))
        }
    }

    /// Gets an `Event` corresponding to a given `rec`.
    pub fn find_event(&self, rec: &Record) -> Result<Event> {
        let handle = self.handle_ref()?;
        let ptr = unsafe { bindings::tep_find_event_by_record(handle.0, rec.0) };
        if ptr.is_null() {
            return Err(Error::FindEvent);
        }
        let name = unsafe { cptr_to_string((*ptr).name) }.expect("string");

        Ok(Event { ptr, name })
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        // Safe because `self.0` must be a valid pointer.
        unsafe {
            bindings::tracecmd_close(self.0);
        }
    }
}

/// A wrapper of
/// [`tep_handle`](https://www.trace-cmd.org/Documentation/libtraceevent/libtraceevent-handle.html),
/// the main structure representing the trace event parser context.
pub struct HandleRef(*mut bindings::tep_handle);

impl HandleRef {
    /// Gets a PID.
    pub fn pid(&self, rec: &Record) -> i32 {
        unsafe { bindings::tep_data_pid(self.0, rec.0) }
    }
}

/// A wrapper of `tep_record`.
pub struct Record(*mut bindings::tep_record);

impl Record {
    /// Gets a timestamp.
    pub fn ts(&self) -> u64 {
        unsafe { *self.0 }.ts
    }
}

/// A wrapper of `tep_event`.
pub struct Event {
    ptr: *mut bindings::tep_event,
    /// Name of the event.
    pub name: String,
}

impl Event {
    /// Prints each field name followed by the record’s field value according to the field’s type.
    ///
    /// This is a wrapper of
    /// [tep_record_print_fields](https://www.trace-cmd.org/Documentation/libtraceevent/libtraceevent-field_print.html).
    pub fn print_fields(&self, rec: &Record) {
        println!("fields: {:?}", self.get_fields(rec));
    }
    /// Get each field name follwed by the record's field value according to the field's type.
    ///
    /// This is a wrapper of
    /// [tep_record_print_fields](https://www.trace-cmd.org/Documentation/libtraceevent/libtraceevent-field_print.html).
    pub fn get_fields(&self, rec: &Record) -> String {
        let mut seq: bindings::trace_seq = Default::default();
        unsafe {
            bindings::trace_seq_init(&mut seq);
            bindings::trace_seq_reset(&mut seq);

            bindings::tep_record_print_fields(&mut seq, rec.0, self.ptr);
            bindings::trace_seq_terminate(&mut seq);
        };
        let msg = unsafe { std::slice::from_raw_parts(seq.buffer as *mut u8, seq.len as usize) };
        std::str::from_utf8(msg).unwrap().to_string()
    }
}

/// A trait to iterate over trace events and process them one by one.
///
/// When you use this trait, you need to implement [Handler::callback] and [Handler::AccumulatedData].
/// Then, you can call [Handler::process] or [Handler::process_multi] to process the given `trace.dat`.
/// When [Handler::process] is called, the defined `callback` is called for each events one by one. The last
///  argument of the `callback` is `&mut Self::AccumulatedData`.
///
/// # Example
///
/// ```no_run
/// use libtracecmd::Event;
/// use libtracecmd::Handler;
/// use libtracecmd::Input;
/// use libtracecmd::Record;
///
/// #[derive(Default)]
/// struct MyData {
///   // fields to accumulate data.
/// }
///
/// impl MyData {
///   fn print_results(&self) {
///     // Print accumulated data.
///   }
/// }
///
/// struct MyStats;
///
/// impl Handler for MyStats {
///   type AccumulatedData = MyData;
///
///   fn callback(input: &mut Input, rec: &mut Record, cpu: i32, data: &mut Self::AccumulatedData) -> i32 {
///     // Write your own logic to analyze `rec` and update `data`.
///     0
///   }
/// }
///
///
/// let mut input: Input = Input::new("trace.dat").unwrap();
/// let stats: MyData = MyStats::process(&mut input).unwrap();
/// stats.print_results();
/// ```
///
/// You can find sample programs in [`/examples/`](https://github.com/google/libtracecmd-rs/tree/main/examples).
pub trait Handler {
    /// Type of data passed around among every call of [Self::callback].
    type AccumulatedData: Default;

    /// A callback that will be called for all events when [Self::process] or [Self::process_multi] is called.
    fn callback(
        input: &mut Input,
        rec: &mut Record,
        cpu: i32,
        data: &mut Self::AccumulatedData,
    ) -> i32;

    /// Processes the given `input` by calling [Self::callback] for each event and returns
    /// [Self::AccumulatedData] returned by the last call of [Self::callback].
    ///
    /// This is a wrapper of [`tracecmd_iterate_events`](https://www.trace-cmd.org/Documentation/libtracecmd/libtracecmd-iterate.html).
    fn process(input: &mut Input) -> std::result::Result<Self::AccumulatedData, i32> {
        let mut data: Self::AccumulatedData = Default::default();

        let ret = unsafe {
            bindings::tracecmd_iterate_events(
                input.0,
                // If `cpus` is null, `cpus` and `cpu_size` are ignored and all of CPUs will be
                // checked.
                std::ptr::null_mut(), /* cpus */
                0,                    /* cpu_size */
                Some(c_callback::<Self>),
                &mut data as *mut _ as *mut std::ffi::c_void,
            )
        };
        if ret == 0 {
            Ok(data)
        } else {
            Err(ret)
        }
    }

    /// Similar to [Self::process], but can take multiple inputs.
    ///
    /// This is useful when you have synchronized multiple trace.dat created by `trace-cmd agent`.
    /// This is a wrapper of [`tracecmd_iterate_events`](https://www.trace-cmd.org/Documentation/libtracecmd/libtracecmd-iterate.html).
    fn process_multi(inputs: &mut [Input]) -> std::result::Result<Self::AccumulatedData, i32> {
        let mut data: Self::AccumulatedData = Default::default();
        let nr_handles = inputs.len() as i32;

        let mut handles = inputs.iter().map(|input| input.0).collect::<Vec<_>>();

        let ret = unsafe {
            bindings::tracecmd_iterate_events_multi(
                handles.as_mut_ptr(),
                nr_handles,
                Some(c_callback::<Self>),
                &mut data as *mut _ as *mut std::ffi::c_void,
            )
        };
        if ret == 0 {
            Ok(data)
        } else {
            Err(ret)
        }
    }
}

unsafe extern "C" fn c_callback<T: Handler + ?Sized>(
    input: *mut bindings::tracecmd_input,
    rec: *mut bindings::tep_record,
    cpu: i32,
    raw_data: *mut std::ffi::c_void,
) -> i32 {
    let mut input = Input(input);
    let mut rec = Record(rec);

    // TODO: Remove this unnecessary data copy?
    // What I only need here is a type conversion.
    let mut data: T::AccumulatedData = Default::default();
    std::ptr::copy_nonoverlapping(
        raw_data,
        &mut data as *mut _ as *mut std::ffi::c_void,
        std::mem::size_of::<T::AccumulatedData>(),
    );
    let res = T::callback(&mut input, &mut rec, cpu, &mut data);
    std::ptr::copy_nonoverlapping(
        &mut data as *mut _ as *mut std::ffi::c_void,
        raw_data,
        std::mem::size_of::<T::AccumulatedData>(),
    );

    std::mem::forget(input);
    std::mem::forget(data);

    res
}
