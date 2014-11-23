use std::error::FromError;
use std::io;
use std::io::fs::mkdir_recursive;

use csv;
use csv::index::Indexed;

use CliResult;
use config::{Config, Delimiter};
use util;

static USAGE: &'static str = "
Splits the given CSV data into chunks.

The files are written to the directory given with the name '{start}.csv',
where {start} is the index of the first record of the chunk (starting at 0).

Usage:
    xsv split [options] <outdir> [<input>]
    xsv split --help

split options:
    -s, --size <arg>       The number of records to write into each chunk.
                           [default: 500]
    -j, --jobs <arg>       The number of spliting jobs to run in parallel.
                           This only works when the given CSV data has
                           an index already created. Note that a file handle
                           is opened for each job.
                           [default: 12]

Common options:
    -h, --help             Display this message
    -o, --output <file>    Write output to <file> instead of stdout.
    -n, --no-headers       When set, the first row will NOT be interpreted
                           as column names. Note that this has no effect when
                           concatenating columns.
    -d, --delimiter <arg>  The field delimiter for reading CSV data.
                           Must be a single character. [default: ,]
";

#[deriving(Clone, Decodable)]
struct Args {
    arg_input: Option<Path>,
    arg_outdir: Path,
    flag_size: uint,
    flag_jobs: uint,
    flag_output: Option<Path>,
    flag_no_headers: bool,
    flag_delimiter: Delimiter,
}

pub fn run(argv: &[&str]) -> CliResult<()> {
    let args: Args = try!(util::get_args(USAGE, argv));
    if args.flag_size == 0 {
        return Err(FromError::from_error("--size must be greater than 0."));
    }
    try!(mkdir_recursive(&args.arg_outdir, io::ALL_PERMISSIONS));

    match try!(args.rconfig().indexed()) {
        Some(idx) => args.parallel_split(idx),
        None => args.sequential_split(),
    }
}

impl Args {
    fn sequential_split(&self) -> CliResult<()> {
        let rconfig = self.rconfig();
        let mut rdr = try!(rconfig.reader());
        let headers = try!(rdr.byte_headers());

        let mut wtr = try!(self.new_writer(headers[], 0));
        for (i, row) in rdr.byte_records().enumerate() {
            if i > 0 && i % self.flag_size == 0 {
                try!(wtr.flush());
                wtr = try!(self.new_writer(headers[], i));
            }
            let row = try!(row);
            try!(wtr.write_bytes(row.into_iter()));
        }
        try!(wtr.flush());
        Ok(())
    }

    fn parallel_split(&self, idx: Indexed<io::File, io::File>)
                     -> CliResult<()> {
        use std::sync::TaskPool;

        let nchunks = util::num_of_chunks(idx.count() as uint, self.flag_size);
        let pool = TaskPool::new(self.flag_jobs);
        for i in range(0, nchunks) {
            let args = self.clone();
            pool.execute(proc() {
                let conf = args.rconfig();
                let mut idx = conf.indexed().unwrap().unwrap();
                let headers = idx.csv().byte_headers().unwrap();
                let mut wtr = args.new_writer(headers[], i * args.flag_size)
                                  .unwrap();

                idx.seek((i * args.flag_size) as u64).unwrap();
                for row in idx.csv().byte_records().take(args.flag_size) {
                    let row = row.unwrap();
                    wtr.write_bytes(row.into_iter()).unwrap();
                }
                wtr.flush().unwrap();
            });
        }
        Ok(())
    }

    fn new_writer(&self, headers: &[csv::ByteString], start: uint)
                 -> CliResult<csv::Writer<Box<io::Writer+'static>>> {
        let path = self.arg_outdir.join(format!("{}.csv", start));
        let mut wtr = try!(Config::new(&Some(path)).writer());
        if !self.flag_no_headers {
            try!(wtr.write_bytes(headers.iter().map(|f| f[])));
        }
        Ok(wtr)
    }

    fn rconfig(&self) -> Config {
        Config::new(&self.arg_input)
               .delimiter(self.flag_delimiter)
               .no_headers(self.flag_no_headers)
    }
}
