use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

fn reader(capacity: Option<usize>) -> NifpgaFastFifoRecv {
    NifpgaFastFifoRecv::new(
        "X",
        "ABCD12345678",
        "RIO0",
        false,
        false,
        0,
        50000,
        8,
        2,
        0,
        false,
        capacity,
    )
    .unwrap()
}

#[test]
fn default_queue_is_unbounded() {
    let mut reader = reader(None);
    assert_eq!(reader.tx.capacity(), None);
    for _ in 0..100 {
        assert!(queue_chunk(&reader.tx, vec![1, 2], &reader.reader_status));
    }
    assert_eq!(reader.get_data_as_list().unwrap().len(), 200);
    let status = lock_reader_status(&reader.reader_status);
    assert_eq!(status.chunks_sent, 100);
    assert_eq!(status.elements_sent, 200);
    assert!(!status.queue_overflow);
}

#[test]
fn full_queue_stops_worker_and_preserves_accepted_data() {
    let mut reader = reader(Some(1));
    let tx = reader.tx.clone();
    let status = reader.reader_status.clone();
    let worker = thread::spawn(move || {
        for chunk in [vec![10, 11], vec![12, 13], vec![14, 15]] {
            if !queue_chunk(&tx, chunk, &status) {
                return;
            }
        }
    });
    worker.join().unwrap();
    assert_eq!(reader.rx.len(), 1);
    {
        let status = lock_reader_status(&reader.reader_status);
        assert!(status.queue_overflow);
        assert_eq!(status.elements_dropped, 2);
        assert_eq!(status.chunks_sent, 1);
        assert_eq!(status.elements_sent, 2);
        assert!(status
            .last_error
            .as_ref()
            .unwrap()
            .contains("FIFO queue overflow"));
    }
    Python::initialize();
    Python::attach(|py| {
        let error = reader.get_data_as_list().unwrap_err();
        assert!(error.is_instance_of::<PyOverflowError>(py));
    });
    // Reporting the error does not consume the data accepted before overflow.
    assert_eq!(reader.rx.len(), 1);
    assert_eq!(reader.get_data_as_list().unwrap(), vec![10, 11]);
    assert!(reader.get_data_as_list().unwrap().is_empty());
    assert!(lock_reader_status(&reader.reader_status).queue_overflow);
}

#[test]
fn every_data_getter_reports_overflow_without_draining() {
    Python::initialize();
    Python::attach(|py| {
        for getter in 0..4 {
            let mut reader = reader(Some(1));
            assert!(queue_chunk(&reader.tx, vec![1, 2], &reader.reader_status));
            assert!(!queue_chunk(&reader.tx, vec![3, 4], &reader.reader_status));
            let error = match getter {
                0 => reader.get_data().unwrap_err(),
                1 => reader.get_data_as_list().unwrap_err(),
                2 => reader.get_data_as_numpy(py).unwrap_err(),
                _ => reader.get_data_as_ntuple(py).unwrap_err(),
            };
            assert!(error.is_instance_of::<PyOverflowError>(py));
            assert_eq!(reader.get_data_as_list().unwrap(), vec![1, 2]);
        }
    });
}

#[test]
fn bounded_queue_can_be_reused_after_drain() {
    let mut reader = reader(Some(1));
    assert!(queue_chunk(&reader.tx, vec![1, 2], &reader.reader_status));
    assert_eq!(reader.get_data_as_list().unwrap(), vec![1, 2]);
    assert!(queue_chunk(&reader.tx, vec![3, 4], &reader.reader_status));
    assert_eq!(reader.get_data_as_list().unwrap(), vec![3, 4]);
    assert!(!lock_reader_status(&reader.reader_status).queue_overflow);
}

#[test]
fn stop_releases_gil_while_joining() {
    Python::initialize();
    Python::attach(|py| {
        let mut reader = reader(None);
        let (request_tx, request_rx) = mpsc::channel();
        let (ack_tx, ack_rx) = mpsc::channel();
        let progressed = Arc::new(AtomicBool::new(false));
        let worker_progressed = progressed.clone();
        // This thread needs the GIL before the simulated reader can finish.
        let python_thread = thread::spawn(move || {
            request_rx.recv().unwrap();
            Python::attach(|_| {
                let _ = ack_tx.send(());
            });
        });
        reader.thread_handle = Some(thread::spawn(move || {
            request_tx.send(()).unwrap();
            worker_progressed.store(
                ack_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
                Ordering::SeqCst,
            );
        }));
        reader.thread_stop(py).unwrap();
        py.detach(move || python_thread.join()).unwrap();
        assert!(progressed.load(Ordering::SeqCst), "shutdown held the GIL");
        assert!(reader.thread_handle.is_none());
        reader.thread_stop(py).unwrap();
    });
}

#[test]
fn early_stop_skips_fpga_initialization() {
    let reader = reader(None);
    *reader.stop_event.lock().unwrap() = true;
    fpga_loop(
        &reader.conf,
        &reader.tx,
        reader.stop_event.clone(),
        reader.reader_status.clone(),
    );
    assert!(*reader.stop_event.lock().unwrap());
    assert!(lock_reader_status(&reader.reader_status)
        .last_error
        .is_none());
}
