use std::error::Error;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use engine_devtools::frame_capture::inspect;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [input, trace, summary] = args.as_slice() else {
        return Err("usage: frame_capture <capture.json> <perfetto.json> <summary.json>".into());
    };
    let input = serde_json::from_reader(BufReader::new(File::open(input)?))?;
    let (timeline, results) = inspect(input)?;
    write(Path::new(trace), &timeline)?;
    write(Path::new(summary), &results)?;
    Ok(())
}

fn write(path: &Path, data: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    let mut output = BufWriter::new(File::create(path)?);
    serde_json::to_writer(&mut output, data)?;
    output.flush()?;
    Ok(())
}
