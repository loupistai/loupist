use super::*;
use std::io::Write;
use zip::{write::SimpleFileOptions, ZipWriter};

const COMPARISON: &[u8] = include_bytes!("comparison.xlsx");

#[test]
fn rejects_malformed_and_oversized_zip_entries() {
    assert!(matches!(
        guard(b"garbled"),
        Err(WorkbookRead::InvalidWorkbook)
    ));
    assert!(guard(COMPARISON).is_ok());
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file(
            "xl/worksheets/sheet1.xml",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    let block = [0; 8192];
    for _ in 0..4097 {
        archive.write_all(&block).unwrap();
    }
    assert!(matches!(
        guard(archive.finish().unwrap().get_ref()),
        Err(WorkbookRead::LimitExceeded)
    ));
}

#[cfg(unix)]
fn large_sheet() -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(COMPARISON)).unwrap();
    let mut dst = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut entry = src.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        dst.start_file(
            &name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        if name == "xl/worksheets/sheet1.xml" {
            write!(dst,"<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>").unwrap();
            for row in 1..=100000 {
                write!(
                    dst,
                    "<row r=\"{row}\"><c r=\"A{row}\"><v>{row}</v></c></row>"
                )
                .unwrap();
            }
            dst.write_all(b"</sheetData></worksheet>").unwrap();
        } else {
            std::io::copy(&mut entry, &mut dst).unwrap();
        }
    }
    dst.finish().unwrap().into_inner()
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires the staged WASI Python runtime"]
async fn workbook_projection_in_the_pinned_worker() {
    let dir = tempfile::tempdir().unwrap();
    let root = BoundLoupistRoot::open_unbound(dir.path().canonicalize().unwrap()).unwrap();
    let cache = WorkbookCache::default();
    let compiler = tempfile::tempdir().unwrap();
    let large = large_sheet();
    // Optionally time a QA export with the same command and cache as Files.
    let qa = std::env::var_os("LOUPIST_WORKBOOK_TIMING_FIXTURE")
        .map(|path| std::fs::read(path).unwrap());
    let mut cases = Vec::new();
    if let Some(bytes) = qa.as_deref() {
        cases.push(("qa-export", bytes, None));
    }
    cases.extend([
        ("comparison", COMPARISON, Some(4)),
        ("100k", large.as_slice(), Some(100000)),
    ]);
    for (label, bytes, total) in cases {
        let path = format!("{label}.xlsx");
        std::fs::write(root.path().join(&path), bytes).unwrap();
        let request = |sheet_index| WorkbookRequest {
            loupist_id: "fixture".into(),
            path: path.clone(),
            sheet_index,
        };
        let start = std::time::Instant::now();
        let cold = read(root.clone(), request(None), &cache, compiler.path(), |_| {})
            .await
            .unwrap();
        let WorkbookRead::Ready(result) = cold else {
            panic!("preview failed: {cold:?}")
        };
        let elapsed = start.elapsed();
        let start = std::time::Instant::now();
        let warm = read(
            root.clone(),
            request(result.selected_sheet),
            &cache,
            compiler.path(),
            |e| assert!(matches!(e, WorkbookProgress::CheckingFile)),
        )
        .await
        .unwrap();
        eprintln!(
            "{label} {} bytes: cold {elapsed:?}, cached {:?}",
            bytes.len(),
            start.elapsed()
        );
        assert!(matches!(warm, WorkbookRead::Ready(_)));
        let Some(total) = total else { continue };
        assert_eq!(result.rows_total, total);
        if label == "100k" {
            assert_eq!(result.rows.len(), 200);
            assert!(result.truncated);
            continue;
        }
        assert_eq!(result.selected_sheet, Some(1));
        assert!(result.sheets[2].hidden);
        let vals: Vec<_> = result.rows[1].iter().map(|c| c.display.as_str()).collect();
        assert_eq!(&vals[..3], &["Issuer A", "12,000", "12.5%"]);
        assert_eq!(&vals[4..], &["TRUE", "#N/A", "24000"]);
        assert!(matches!(result.rows[1][3].kind, WorkbookCellKind::Date));
        assert!(result.rows[1][6].formula);
        assert_eq!(result.rows[3][0].display, "Merged");
        assert!(matches!(result.rows[3][1].kind, WorkbookCellKind::Empty));
        assert!(result
            .omitted
            .iter()
            .any(|o| matches!(o, WorkbookOmission::Charts)));
        let hidden = read(
            root.clone(),
            request(Some(2)),
            &cache,
            compiler.path(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(matches!(hidden, WorkbookRead::UnsupportedSheet));
    }
}
