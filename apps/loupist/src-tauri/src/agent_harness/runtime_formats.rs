//! Exercise the file operations advertised by the bundled Skills in the actual
//! pinned worker, rather than importing libraries in the developer's Python.
use serde_json::Value;
use std::sync::OnceLock;

static WORKER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn probe(code: &str) -> (tempfile::TempDir, Value) {
    let _worker = WORKER.lock().await;
    static CACHE: OnceLock<tempfile::TempDir> = OnceLock::new();
    let temporary = tempfile::tempdir().unwrap();
    let base = temporary.path();
    for name in ["workspace", "app", "scratch", "attachments"] {
        std::fs::create_dir(base.join(name)).unwrap();
    }
    let roots = super::native_policy::NativeRoots::new(
        &base.join("workspace"),
        &base.join("app"),
        &base.join("app"),
        &base.join("workspace/.loupist"),
        &base.join("scratch"),
        &base.join("attachments"),
        None,
    );
    let cache = CACHE.get_or_init(|| tempfile::tempdir().unwrap());
    let result = crate::python_run::execute(
        &roots,
        cache.path(),
        code,
        120,
        crate::web::SearchCancel::never(),
    )
    .await
    .unwrap();
    (temporary, result)
}

#[tokio::test]
async fn bundled_file_skills_roundtrip_in_the_pinned_wasi_worker() {
    let (temporary, result) = probe(
        r#"
from pathlib import Path
from docx import Document
from pptx import Presentation
from openpyxl import Workbook, load_workbook
from reportlab.pdfgen import canvas
from pypdf import PdfReader, PdfWriter
import pandas as pd
import struct, xlrd, xlsxwriter, pdfplumber
out = Path('/workspace/outputs')
doc = Document()
doc.add_heading('Verified document', 0)
doc.add_table(rows=1, cols=1).cell(0, 0).text = '42'
doc.save(out / 'report.docx')
assert Document(out / 'report.docx').tables[0].cell(0, 0).text == '42'
deck = Presentation()
slide = deck.slides.add_slide(deck.slide_layouts[0])
slide.shapes.title.text = 'Verified slide'
deck.save(out / 'report.pptx')
assert Presentation(out / 'report.pptx').slides[0].shapes.title.text == 'Verified slide'
book = Workbook()
book.active['A1'] = 42
book.active['A2'] = '=A1*2'
book.save(out / 'report.xlsx')
assert load_workbook(out / 'report.xlsx').active['A2'].value == '=A1*2'
assert load_workbook(out / 'report.xlsx', data_only=True).active['A2'].value is None
book = xlsxwriter.Workbook(str(out / 'new.xlsx'))
book.add_worksheet().write(0, 0, 42)
book.close()
assert load_workbook(out / 'new.xlsx').active['A1'].value == 42
# A minimal BIFF8 workbook verifies legacy XLS parsing without a host office app.
record = lambda opcode, data: struct.pack('<HH', opcode, len(data)) + data
bof = lambda kind: record(0x0809, struct.pack('<HHHHII', 0x0600, kind, 0x0DBB, 0x07CC, 0x41, 6))
eof = record(0x000A, b'')
name = b'Data'
bounds = lambda offset: record(0x0085, struct.pack('<IBBBB', offset, 0, 0, len(name), 0) + name)
header = bof(0x0005) + record(0x0042, struct.pack('<H', 1200))
offset = len(header) + len(bounds(0)) + len(eof)
raw = header + bounds(offset) + eof + bof(0x0010)
raw += record(0x0200, struct.pack('<IIHHH', 0, 1, 0, 1, 0))
raw += record(0x0203, struct.pack('<HHHd', 0, 0, 0, 42.0)) + eof
(out / 'legacy.xls').write_bytes(raw)
assert xlrd.open_workbook(out / 'legacy.xls', use_mmap=False).sheet_by_index(0).cell_value(0, 0) == 42
for suffix, separator in [('csv', ','), ('tsv', '\t')]:
    path = out / ('table.' + suffix)
    pd.DataFrame({'amount': [1, 2]}).to_csv(path, sep=separator, index=False)
    assert pd.read_csv(path, sep=separator)['amount'].sum() == 3
pdf = canvas.Canvas(str(out / 'report.pdf'))
pdf.drawString(72, 720, 'Verified PDF')
pdf.save()
reader = PdfReader(out / 'report.pdf')
assert 'Verified PDF' in reader.pages[0].extract_text()
writer = PdfWriter()
writer.add_page(reader.pages[0])
writer.write(out / 'copy.pdf')
assert len(PdfReader(out / 'copy.pdf').pages) == 1
assert 'Verified PDF' in PdfReader(out / 'copy.pdf').pages[0].extract_text()
pdf = canvas.Canvas(str(out / 'table.pdf'))
for x in [72, 172, 272]:
    pdf.line(x, 680, x, 720)
for y in [680, 700, 720]:
    pdf.line(72, y, 272, y)
for x, y, text in [(82, 705, 'Metric'), (182, 705, 'Value'), (82, 685, 'Units'), (182, 685, '42')]:
    pdf.drawString(x, y, text)
pdf.save()
with pdfplumber.open(out / 'table.pdf') as source:
    assert source.pages[0].extract_table() == [['Metric', 'Value'], ['Units', '42']]
print('file-skills-roundtrip-ok')
"#,
    )
    .await;
    let base = temporary.path();
    assert_eq!(result["exit_code"], 0, "{result}");
    assert!(
        result["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("file-skills-roundtrip-ok"),
        "{result}"
    );
    for name in [
        "report.docx",
        "report.pptx",
        "report.xlsx",
        "new.xlsx",
        "legacy.xls",
        "table.pdf",
        "report.pdf",
        "copy.pdf",
        "table.csv",
        "table.tsv",
    ] {
        assert!(
            base.join("workspace/outputs").join(name).is_file(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn every_advertised_python_library_imports_in_the_pinned_worker() {
    for modules in [
        "numpy,pandas,scipy,statsmodels,sklearn,matplotlib,seaborn,sympy,networkx,xarray,openpyxl,xlsxwriter",
        "xlrd,docx,pptx,pypdf,pdfplumber,reportlab,PIL,lxml,bs4,yaml,tabulate,jinja2",
    ] {
        let code = format!("import importlib\nfor name in '{}'.split(','):\n    importlib.import_module(name)\n    print(name)\n", modules);
        let (_temporary, result) = probe(&code).await;
        assert_eq!(result["exit_code"], 0, "{result}");
        for name in modules.split(',') {
            assert!(result["stdout"].as_str().unwrap_or_default().lines().any(|line| line == name), "{name}: {result}");
        }
    }
}
