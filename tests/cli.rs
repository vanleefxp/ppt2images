//! End-to-end tests for the native backend and output naming.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use zip::write::SimpleFileOptions;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
  <Override PartName="/ppt/slides/slide2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#;

const PRESENTATION: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
    xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
    xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:sldIdLst>
    <p:sldId id="256" r:id="rId2"/>
    <p:sldId id="257" r:id="rId3"/>
  </p:sldIdLst>
  <p:sldSz cx="12192000" cy="6858000"/>
  <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#;

const PRESENTATION_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/>
</Relationships>"#;

fn slide_xml(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
    xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
    xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:bg><p:bgPr><a:solidFill><a:srgbClr val="1F2937"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>
    <p:spTree>
      <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr/>
      <p:sp>
        <p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr>
        <p:spPr>
          <a:xfrm><a:off x="914400" y="914400"/><a:ext cx="10363200" cy="2286000"/></a:xfrm>
          <a:prstGeom prst="rect"><a:avLst/></a:prstGeom>
        </p:spPr>
        <p:txBody>
          <a:bodyPr/><a:lstStyle/>
          <a:p><a:r><a:rPr lang="en-US" sz="4000" b="1"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:rPr><a:t>{title}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sld>"#
    )
}

fn write_pptx(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let parts = [
        ("[Content_Types].xml", CONTENT_TYPES.to_owned()),
        ("_rels/.rels", ROOT_RELS.to_owned()),
        ("ppt/presentation.xml", PRESENTATION.to_owned()),
        (
            "ppt/_rels/presentation.xml.rels",
            PRESENTATION_RELS.to_owned(),
        ),
        ("ppt/slides/slide1.xml", slide_xml("First slide")),
        ("ppt/slides/slide2.xml", slide_xml("Second slide")),
    ];
    for (name, contents) in parts {
        archive.start_file(name, options).unwrap();
        archive.write_all(contents.as_bytes()).unwrap();
    }
    archive.finish().unwrap();
}

fn fixture() -> (TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("deck.pptx");
    write_pptx(&input);
    (directory, input)
}

fn run_with_size(
    input: &Path,
    output: &Path,
    size_args: &[&str],
    extra: &[&str],
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ppt2images"));
    command
        .arg(input)
        .arg("--backend")
        .arg("native")
        .arg("--output-dir")
        .arg(output)
        .args(size_args)
        .args(extra);
    command.output().unwrap()
}

fn run(input: &Path, output: &Path, extra: &[&str]) -> std::process::Output {
    run_with_size(input, output, &["--resolution", "320x180"], extra)
}

fn assert_image_size(path: &Path, width: u32, height: u32) {
    assert_eq!(
        image::open(path).unwrap().to_rgb8().dimensions(),
        (width, height)
    );
}

#[test]
fn converts_a_pptx_with_custom_numbering() {
    let (directory, input) = fixture();
    let output = directory.path().join("frames");
    let result = run(
        &input,
        &output,
        &["--page-format", "slide-{page:03}", "--start-index", "2"],
    );
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let first = output.join("slide-002.png");
    let second = output.join("slide-003.png");
    assert!(first.is_file());
    assert!(second.is_file());
    assert_image_size(&first, 320, 180);
    assert_image_size(&second, 320, 180);
    assert!(!output.join("0000.png").exists());
    let names = fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
}

#[test]
fn refuses_to_replace_outputs_without_overwrite() {
    let (directory, input) = fixture();
    let output = directory.path().join("frames");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("0000.png"), b"keep").unwrap();
    let result = run(&input, &output, &[]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("--overwrite"), "stderr: {stderr}");
    assert_eq!(fs::read(output.join("0000.png")).unwrap(), b"keep");
}

#[test]
fn overwrite_publishes_a_complete_new_set() {
    let (directory, input) = fixture();
    let output = directory.path().join("frames");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("0000.png"), b"old").unwrap();
    fs::write(output.join("0001.png"), b"old").unwrap();
    let result = run(&input, &output, &["--overwrite"]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    for name in ["0000.png", "0001.png"] {
        assert!(image::open(output.join(name)).is_ok());
    }
}

#[test]
fn derives_height_from_width_and_width_from_height() {
    let (directory, input) = fixture();

    let width_output = directory.path().join("width");
    let result = run_with_size(&input, &width_output, &["--width", "320"], &[]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_image_size(&width_output.join("0000.png"), 320, 180);

    let height_output = directory.path().join("height");
    let result = run_with_size(&input, &height_output, &["--height", "180"], &[]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_image_size(&height_output.join("0000.png"), 320, 180);
}

#[test]
fn derives_size_from_dpi() {
    let (directory, input) = fixture();
    let output = directory.path().join("dpi");
    let result = run_with_size(&input, &output, &["--dpi", "24"], &[]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_image_size(&output.join("0000.png"), 320, 180);
}
