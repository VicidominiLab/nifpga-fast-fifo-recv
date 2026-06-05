extern crate nifpga_dll;

use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3::types::PyTuple;

use numpy::PyArray1;
//use numpy::{PyArray1, PyArray2, PyReadonlyArray1};
// use rayon::prelude::*;

use crossbeam;
use nifpga_dll::Session;
use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
#[pyclass]
struct Configuration {
    bit_file: String,
    signature: String,
    ni_address: String,
    run: bool,
    close_on_reset: bool,
    fifo: u32,
    dma_buffer_size: usize,
    fifo_reading_buffer: usize,
    min_packet: usize,
    delay_us: u64,
    debug: bool,
}

#[derive(Clone, Default)]
struct ReaderStatus {
    chunks_sent: u64,
    elements_sent: u64,
    last_error: Option<String>,
}

fn round_down_to_packet(elements: usize, min_packet: usize) -> usize {
    (elements / min_packet) * min_packet
}

fn lock_reader_status(status: &Arc<Mutex<ReaderStatus>>) -> MutexGuard<'_, ReaderStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_reader_error(status: &Arc<Mutex<ReaderStatus>>, message: String) {
    eprintln!("{}", message);
    lock_reader_status(status).last_error = Some(message);
}

fn fpga_loop(
    conf: &Configuration,
    tx: &crossbeam::channel::Sender<Vec<u64>>,
    stop_event: Arc<Mutex<bool>>,
    reader_status: Arc<Mutex<ReaderStatus>>,
) {
    if conf.min_packet == 0 {
        set_reader_error(
            &reader_status,
            "min_packet must be greater than 0".to_string(),
        );
        return;
    }

    let chunk_len = round_down_to_packet(conf.fifo_reading_buffer, conf.min_packet);
    if chunk_len == 0 {
        set_reader_error(
            &reader_status,
            "fifo_reading_buffer must be at least min_packet".to_string(),
        );
        return;
    }

    let session = match Session::open(
        conf.bit_file.as_str(),
        conf.signature.as_str(), //signature from generated header
        conf.ni_address.as_str(),
        conf.run,            //run on open
        conf.close_on_reset, //close_on_reset on close
    ) {
        Ok(session) => session,
        Err(error) => {
            set_reader_error(
                &reader_status,
                format!("Failed to open FPGA session: {}", error),
            );
            return;
        }
    };

    let (reader, depth) = match session.open_read_fifo::<u64>(conf.fifo, conf.dma_buffer_size) {
        Ok(fifo) => fifo,
        Err(error) => {
            set_reader_error(
                &reader_status,
                format!("Failed to open FIFO {}: {}", conf.fifo, error),
            );
            return;
        }
    };

    println!(
        "Actual DMA FIFO  {} set depth: {} actual depth: {}",
        conf.fifo, conf.dma_buffer_size, depth
    );
    println!("conf.fifo_reading_buffer: {}", conf.fifo_reading_buffer);
    println!("debug: {}", conf.debug);

    let mut read_buff: Vec<u64> = vec![0; chunk_len];

    let mut read_buff_zero_size: Vec<u64> = Vec::with_capacity(0);
    let mut elements_remaining = 0;

    let debug = conf.debug;
    // let debug = false;

    // let mut now_time = Instant::now();

    *stop_event.lock().unwrap() = false;

    loop {
        if *stop_event.lock().unwrap() {
            break;
        }

        // let last_time = Instant::now();
        if elements_remaining == 0 {
            if conf.delay_us > 0 {
                std::thread::sleep(Duration::from_micros(conf.delay_us));
            };
            elements_remaining = match reader.read(&mut read_buff_zero_size, 0) {
                Ok(remaining) => round_down_to_packet(remaining, conf.min_packet),
                Err(error) => {
                    set_reader_error(
                        &reader_status,
                        format!("FIFO availability poll failed: {}", error),
                    );
                    break;
                }
            };

            if debug == true {
                if (elements_remaining) > 0 {
                    println!("elements_remaining was 0 now :{}", elements_remaining);
                }
            }
        }

        if elements_remaining > 0 {
            if debug == true {
                if (elements_remaining) > 0 {
                    println!("elements_remaining was > 0 now :{}", elements_remaining);
                }
            }

            let elements_to_read = std::cmp::min(elements_remaining, chunk_len);
            read_buff.resize(elements_to_read, 0);

            let remaining_after_read: usize = match reader.read(&mut read_buff, 0) {
                Ok(remaining) => remaining,
                Err(error) => {
                    set_reader_error(&reader_status, format!("FIFO read failed: {}", error));
                    break;
                }
            };

            if debug == true {
                if (elements_remaining) > 0 {
                    println!("elements_to_read:{}", elements_to_read);
                }
            }

            elements_remaining = round_down_to_packet(remaining_after_read, conf.min_packet);

            if debug == true {
                if (elements_remaining) > 0 {
                    println!("elements_remaining after read:{}", elements_remaining);
                }
            }

            if elements_to_read > 0 {
                if let Err(error) = tx.send(read_buff[..elements_to_read].to_vec()) {
                    set_reader_error(
                        &reader_status,
                        format!("Failed to send FIFO data to queue: {}", error),
                    );
                    break;
                }

                let mut status = lock_reader_status(&reader_status);
                status.chunks_sent = status.chunks_sent.saturating_add(1);
                status.elements_sent = status.elements_sent.saturating_add(elements_to_read as u64);

                if debug == true {
                    println!("remaining_after_read {}", remaining_after_read);
                }
            }
        }

        // now_time = Instant::now();
        // let delta_time
        //     = now_time - last_time;
    }
}

/// Reading out from NI FPGA FIFOs with a separate thread allowing fast data-rate in Python.
///
/// It is implemented in Rust and it provides in Python the handle to start a separate thread running
/// a continuously polling loop, store the received data in a queue and reading them asynchronously.
/// This allow to receive data from nifpga in Python without loosing data even for fast data-rate.
///
#[pymodule]
fn nifpga_fast_fifo_recv(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    m.add_class::<NifpgaFastFifoRecv>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("__doc__", env!("CARGO_PKG_DESCRIPTION"))?;

    m.add(
        "__all__",
        vec!["NifpgaFastFifoRecv", "__version__", "__doc__"],
    )?;

    Ok(())
}

#[pyclass]
struct NifpgaFastFifoRecv {
    conf: Configuration,
    tx: crossbeam::channel::Sender<Vec<u64>>,
    rx: crossbeam::channel::Receiver<Vec<u64>>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    stop_event: Arc<Mutex<bool>>,
    reader_status: Arc<Mutex<ReaderStatus>>,
}

///provaC
#[pymethods]
impl NifpgaFastFifoRecv {
    ///Create a new NifpgaFastFifoRecv
    ///NifpgaFastFifoRecv(bitfile, signature, run, close_on_reset, fifo, dma_buffer_size,
    ///fifo_reading_buffer, fifo_buffer_size)
    #[new]
    #[pyo3(signature = (bitfile = "X", signature = "ABCD12345678", ni_address = "RI0", run = false, close_on_reset = false, fifo = 0, dma_buffer_size=50000, fifo_reading_buffer=10000, min_packet=1, delay_us = 0, debug = false))]
    fn new(
        bitfile: &str,
        signature: &str,
        ni_address: &str,
        run: bool,
        close_on_reset: bool,
        fifo: u32,
        dma_buffer_size: usize,
        fifo_reading_buffer: usize,
        min_packet: usize,
        delay_us: u64,
        debug: bool,
    ) -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        NifpgaFastFifoRecv {
            conf: Configuration {
                bit_file: String::from(bitfile),
                signature: String::from(signature),
                ni_address: String::from(ni_address),
                run,
                close_on_reset,
                fifo,
                dma_buffer_size,
                fifo_reading_buffer,
                min_packet,
                delay_us,
                debug,
            },
            tx: tx,
            rx: rx,
            thread_handle: None::<std::thread::JoinHandle<()>>,
            stop_event: Arc::new(Mutex::new(false)),
            reader_status: Arc::new(Mutex::new(ReaderStatus::default())),
        }
    }

    ///thread_start run the thread
    fn thread_start(&mut self, _py: Python<'_>) -> PyResult<()> {
        println!("Start two threads.");
        let conf_local = self.conf.clone();
        let tx_local = self.tx.clone();
        let stop_event = self.stop_event.clone();
        let reader_status = self.reader_status.clone();

        match self.thread_handle.take() {
            Some(handle) if handle.is_finished() => {
                if handle.join().is_err() {
                    lock_reader_status(&self.reader_status).last_error =
                        Some("FIFO reader thread panicked".to_string());
                }
            }
            Some(handle) => {
                self.thread_handle = Some(handle);
                eprintln!("THREAD ALREADY EXITS");
                return Ok(());
            }
            None => {}
        };

        {
            let mut status = lock_reader_status(&self.reader_status);
            *status = ReaderStatus::default();
        }

        {
            let handle = thread::spawn(move || {
                //fpga_loop_dummy(&conf_local, &tx_local, stop_event);
                fpga_loop(&conf_local, &tx_local, stop_event, reader_status)
            });

            println!("Thread started!");

            self.thread_handle = Some(handle);
        }

        Ok(())
    }

    ///thread_is_running return true is thread is running
    ///
    /// returns a bool (PyO3 will convert to Python bool)
    fn thread_is_running(&mut self) -> PyResult<bool> {
        let status = match &self.thread_handle {
            Some(handle) => !handle.is_finished(),
            None => false,
        };
        Ok(status)
    }

    ///thread_stop stop the thread
    fn thread_stop(&mut self, _py: Python<'_>) -> PyResult<()> {
        *self.stop_event.lock().unwrap() = true;
        println!("stop_event True");

        if let Some(handle) = self.thread_handle.take() {
            if handle.join().is_err() {
                lock_reader_status(&self.reader_status).last_error =
                    Some("FIFO reader thread panicked".to_string());
            }
        } else {
            eprintln!("THREAD DO NOT EXITS");
        }

        self.thread_handle = None::<std::thread::JoinHandle<()>>;

        // match th {
        println!("stop_event True done");
        Ok(())
    }

    fn get_data(&mut self) -> PyResult<Vec<u64>> {
        self.get_data_as_list()
    }

    // /// Get data as a NumPy array
    fn get_data_as_numpy<'py>(&mut self, py: Python<'py>) -> PyResult<Py<PyArray1<u64>>> {
        let debug = self.conf.debug;

        // Pre-allocate with estimated capacity to reduce reallocations
        let mut combined: Vec<u64> = Vec::new();

        // Collect all items efficiently
        while let Ok(item) = self.rx.try_recv() {
            combined.extend(item);
        }

        if debug {
            println!("combined.len() = {}", combined.len());
        }

        // Zero-copy: transfer ownership directly to NumPy without cloning
        let array = PyArray1::from_vec(py, combined);
        Ok(array.into())
    }

    /// Get data as a Python tuple
    fn get_data_as_ntuple<'py>(&mut self, py: Python<'py>) -> PyResult<Py<PyTuple>> {
        let debug = self.conf.debug;
        let mut combined: Vec<u64> = Vec::new();

        while let Ok(item) = self.rx.try_recv() {
            combined.extend(item);
        }

        if debug {
            println!("combined.len() = {}", combined.len());
        }

        // PyTuple::new returns Result<Bound<PyTuple>, PyErr>, so unwrap it via ? first
        let tuple = PyTuple::new(py, combined)?;
        Ok(tuple.into()) // now we can convert Bound<PyTuple> -> Py<PyTuple>
    }

    /// Get data from the internal queue
    ///
    /// Drains the channel directly into a single Vec<u64>
    /// Returns a Python list of ints (PyO3 will convert automatically)
    fn get_data_as_list(&mut self) -> PyResult<Vec<u64>> {
        let debug = self.conf.debug;
        let mut combined: Vec<u64> = Vec::new();

        // Drain the channel directly into a single vector
        while let Ok(item) = self.rx.try_recv() {
            combined.extend(item); // assuming item: Vec<u64>
        }

        if debug {
            println!("combined.len() = {}", combined.len());
        }

        Ok(combined) // PyO3 converts Vec<u64> -> Python list automatically
    }

    ///get current reader thread and queue diagnostics
    fn get_status(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let d = PyDict::new(py);
        let running = match &self.thread_handle {
            Some(handle) => !handle.is_finished(),
            None => false,
        };
        let status = lock_reader_status(&self.reader_status);

        d.set_item("running", running)?;
        d.set_item("queued_chunks", self.rx.len())?;
        d.set_item("chunks_sent", status.chunks_sent)?;
        d.set_item("elements_sent", status.elements_sent)?;
        d.set_item("last_error", status.last_error.clone())?;

        Ok(d.into())
    }

    ///get the current configuration
    /// returns a Python dict with all configuration fields
    fn get_conf(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let d = PyDict::new(py);

        d.set_item("bit_file", &self.conf.bit_file)?;
        d.set_item("signature", &self.conf.signature)?;
        d.set_item("ni_address", &self.conf.ni_address)?;
        d.set_item("run", self.conf.run)?;
        d.set_item("close_on_reset", self.conf.close_on_reset)?;
        d.set_item("fifo", self.conf.fifo)?;
        d.set_item("dma_buffer_size", self.conf.dma_buffer_size)?;
        d.set_item("fifo_reading_buffer", self.conf.fifo_reading_buffer)?;
        d.set_item("delay_us", self.conf.delay_us)?;
        d.set_item("min_packet", self.conf.min_packet)?;

        Ok(d.into())
    }

    ///get the current configuration
    ///
    /// returns a tuple:
    /// (bit_file, signature, ni_address, run, close_on_reset, fifo, dma_buffer_size, fifo_reading_buffer, delay_us, min_packet)
    /// Python callers can unpack or convert to dict as they prefer.
    fn get_conf_as_tuple(
        &mut self,
    ) -> PyResult<(
        String,
        String,
        String,
        bool,
        bool,
        u32,
        usize,
        usize,
        u64,
        usize,
    )> {
        Ok((
            self.conf.bit_file.clone(),
            self.conf.signature.clone(),
            self.conf.ni_address.clone(),
            self.conf.run,
            self.conf.close_on_reset,
            self.conf.fifo,
            self.conf.dma_buffer_size,
            self.conf.fifo_reading_buffer,
            self.conf.delay_us,
            self.conf.min_packet,
        ))
    }
}
