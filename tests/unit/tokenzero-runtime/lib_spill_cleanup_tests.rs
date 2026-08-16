    use super::*;
    use std::io::Read;

    struct FailAfter {
        data: Vec<u8>,
        pos: usize,
        reads: usize,
        fail_at: usize,
    }

    impl Read for FailAfter {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            if self.reads >= self.fail_at {
                return Err(io::Error::other("injected read failure"));
            }
            let remain = self.data.len().saturating_sub(self.pos);
            let n = remain.min(buf.len());
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    #[test]
    fn capture_error_unlinks_partial_spill() {
        let dir = tempfile::tempdir().unwrap();
        let policy = RunOutputPolicy {
            per_stream_capture_bytes: 64,
            spill_threshold_bytes: 8,
            spill_dir: Some(dir.path().to_path_buf()),
        };
        let reader = FailAfter {
            data: vec![b'x'; 32],
            pos: 0,
            reads: 0,
            fail_at: 2,
        };
        capture_reader_with_observer(reader, "stdout", policy, |_| {})
            .expect_err("injected read failure");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name();
                let name = name.to_string_lossy();
                (name.starts_with("tokenzero-") && name.ends_with(".log")).then(|| name.into_owned())
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "failed capture must unlink spill files: {leftovers:?}"
        );
    }

