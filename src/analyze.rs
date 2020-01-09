use crate::label::*;
use crate::pretty;
use crate::summarize;
use anyhow::*;
use std::time::*;
use structopt::*;

#[derive(StructOpt)]
pub struct Options {
    // /// The target CI width.  Applies to the 95% CI; units are percent of base.
    // #[structopt(long)]
    // threshold: Option<f64>,
    #[structopt(long)]
    deny_positive: bool,
    /// A "base" label.  If specified, all labels will be compared to this.
    #[structopt(long)]
    pub base: Option<String>,
    /// Benchs to compare.  If "base" is not specified, they'll be compared
    /// consecutively.
    pub labels: Vec<String>,
}
impl Options {
    pub fn pairs(&self) -> Vec<(Bench, Bench)> {
        if let Some(base) = &self.base {
            let base = Bench::from(base.as_str());
            self.labels
                .iter()
                .map(|x| (base, Bench::from(x.as_str())))
                .collect()
        } else {
            let iter = self.labels.iter().map(|x| Bench::from(x.as_str()));
            iter.clone().zip(iter.skip(1)).collect()
        }
    }
}

// summarize -> rate-limit -> diff -> pretty print
pub fn analyze(opts: Options) -> Result<()> {
    let mut rdr = csv::Reader::from_reader(std::io::stdin());
    let all_metrics = rdr
        .headers()
        .unwrap()
        .into_iter()
        .skip(1)
        .map(|x| Metric::from(x))
        .collect::<Vec<_>>();
    let mut measurements = summarize::Measurements::new(&all_metrics);

    let mut printer = Printer::new()?;
    let explicit_pairs = opts.pairs();
    macro_rules! print {
        () => {{
            let pairs = if explicit_pairs.is_empty() {
                measurements.guess_pairs()
            } else {
                explicit_pairs.clone()
            };
            let mut diffs = vec![];
            for (from, to) in pairs {
                let diff = measurements.diff(from.clone(), to.clone());
                diffs.push((from, to, diff));
            }
            let out = pretty::render(&all_metrics, &measurements, &diffs)?;
            printer.print(out)?;
            diffs
        }};
    }

    let mut last_print = Instant::now();
    for row in rdr.into_records() {
        let row = row?;
        let mut row = row.into_iter();
        let label = Bench::from(row.next().unwrap());
        let values = row.map(|x| x.parse().unwrap());
        measurements.update(label, all_metrics.iter().cloned().zip(values));

        if last_print.elapsed() > Duration::from_millis(100) {
            last_print = Instant::now();
            print!();

            // // Check to see if we're finished
            // if let Some(threshold) = opts.threshold {
            //     let worst = diff
            //         .diffs
            //         .iter()
            //         .flat_map(|diff| stats.iter().map(move |stat| *diff.cis.get(stat)?))
            //         .map(|x| x.map_or(std::f64::INFINITY, |x| x.r95_pc()))
            //         .fold(std::f64::NEG_INFINITY, f64::max);
            //     if worst < threshold {
            //         break;
            //     } else {
            //         info!("Threshold not reached: {}% > {}%", worst, threshold);
            //     }
            // }
        }
    }

    // Print the last set of diffs
    let diffs = print!();

    if opts.deny_positive {
        for (from, to, diff) in diffs {
            for (idx, ci) in diff.0.into_iter().enumerate() {
                let metric = Metric(idx);
                if ci.delta() > ci.ci(0.95) {
                    bail!("{}..{}: {} increased!", from, to, metric);
                }
            }
        }
    }

    Ok(())
}

pub struct Printer {
    stdout: Box<term::StdoutTerminal>,
    /// The number of lines output in the previous iteration
    n: usize,
}
impl Printer {
    pub fn new() -> Result<Printer> {
        Ok(Printer {
            stdout: term::stdout().ok_or_else(|| anyhow!("Couldn't open stdout as a terminal"))?,
            n: 0,
        })
    }
    // Clear the previous output and replace it with the new output
    pub fn print(&mut self, out: Vec<u8>) -> Result<()> {
        for _ in 0..self.n {
            self.stdout.cursor_up()?;
            self.stdout.delete_line()?;
        }
        self.stdout.write_all(&out)?;
        self.n = out.into_iter().filter(|c| *c == b'\n').count();
        Ok(())
    }
}