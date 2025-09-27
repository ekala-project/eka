//! logging stubs for consistent progress and task presentation
use tracing::Span;
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

/// Set up the given span to be styled as a subtask of another span
pub fn set_sub_task(span: &Span, msg: &str) {
    span.pb_set_style(
        &ProgressStyle::with_template("  {span_child_prefix} {spinner:.blue} {wide_msg}")
            .unwrap_or(ProgressStyle::default_spinner()),
    );
    span.pb_set_message(msg);
}

/// Set up the given span to be styled as a progress bar
pub fn set_bar(span: &Span, msg: &str, len: u64) {
    let style = ProgressStyle::with_template(
        "{elapsed} ░ {prefix} ░ {bar:30.green/black} {percent}% ░ {msg}",
    )
    .unwrap_or(ProgressStyle::default_bar())
    .progress_chars("█▒ ");
    span.pb_set_style(&style);
    span.pb_set_message(msg);
    span.pb_set_length(len);
}

/// given a size hint, calculate the best size
pub fn best_size(size_hint: (usize, Option<usize>)) -> usize {
    match size_hint {
        (l, None) => l,
        (_, Some(u)) => u,
    }
}
