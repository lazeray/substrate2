use std::fmt::Display;

use tracing::{event, span, Level, Span};

#[derive(Debug, Clone)]
pub struct SubstrateError {
    span: Span,
    message: String,
}


impl Display for SubstrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub type Result<T> = std::result::Result<T, SubstrateError>;

pub fn ctx_generate_sram() -> Result<()> {
    if let Err(err) = generate_sram() {
        let guard = err.span.enter();
        event!(Level::ERROR, %err, "failed to generate");
        drop(guard);
        return Err(err);
    }

    // continue doing stuff with our newly generated sram
    Ok(())
}

pub fn generate_sram() -> Result<()> {
    let span = span!(Level::INFO, "generate sram");
    let _guard = span.enter();

    generate_bitcell_array()?;
    event!(Level::WARN, "found floating net `dout[4]` in schematic");
    event!(Level::INFO, "running auto routing");
    event!(Level::INFO, "auto routing finished in 15.47 seconds");

    Ok(())
}

fn autoroute() -> Result<()> {
    let span = span!(Level::INFO, "autoroute");
    let _guard = span.enter();

    let err = SubstrateError {
        span: span.clone(),
        message: String::from("no autoroute route found"),
    };

    Err(err)
}

pub fn generate_bitcell_array() -> Result<()> {
    let span = span!(Level::INFO, "generate bitcell_array");
    let _guard = span.enter();

    event!(Level::INFO, "reading sram_sp_cell from sram_sp_cell.gds");
    event!(
        Level::INFO,
        line = line!(),
        "reading sram_sp_hstrap from sram_sp_hstrap.gds"
    );

    // these "line" spans can be generated by #[substrate::instrument]
    let span = span!(Level::INFO, "line", line = line!());
    let guard = span.enter();
    let _err = autoroute();
    drop(guard);
    // we did some cool error handling and autoroute failure isnt a problem

    // this autoroute better work, or else we fail
    let span = span!(Level::INFO, "line", line = line!());
    let guard = span.enter();
    autoroute()?;
    drop(guard);

    Ok(())
}

fn hi() {
    // enter span
    // let res = hi_inner()
    // if res = err {
    // print err
    // }
    // return res
}

fn hi_inner() {

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing() {
        tracing_subscriber::fmt()
            .with_line_number(true)
            .with_file(true)
            .with_target(true)
            .with_max_level(Level::DEBUG)
            .pretty()
            .init();
        ctx_generate_sram().expect("failed to generate SRAM");
    }
}
