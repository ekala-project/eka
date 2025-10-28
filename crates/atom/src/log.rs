//! # Logging Utilities
//!
//! This module provides utility functions for creating and styling progress indicators
//! and spinners in the console, ensuring a consistent look and feel for logging across
//! the application.
//!
//! ## Overview
//!
//! The logging system integrates with `tracing` and `tracing_indicatif` to associate
//! progress bars and spinners with logging spans. This provides visual feedback during
//! long-running operations like package resolution, publishing, and dependency fetching.
//!
//! ## Key Functions
//!
//! - [`best_size`] - Calculates optimal size hints for progress bars
//! - [`set_bar`] - Configures progress bars with consistent styling
//! - [`set_sub_task`] - Sets up sub-task spinners for nested operations
//!
//! ## Usage
//!
//! ```rust,no_run
//! use atom::log::{set_bar, set_sub_task};
//! use tracing::{Instrument, info_span};
//!
//! let span = info_span!("operation");
//! let _enter = span.enter();
//!
//! // Set up a progress bar
//! set_bar(&span, "Processing items", 100);
//!
//! // Or set up a sub-task spinner
//! set_sub_task(&span, "Fetching dependencies");
//! ```

use tracing::Span;
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

//================================================================================================
// Functions
//================================================================================================

/// Calculates the best size from an iterator's size hint.
///
/// This function prefers the upper bound of a size hint if it exists,
/// otherwise it falls back to the lower bound. This is useful for pre-allocating
/// capacity or setting progress bar lengths.
pub fn best_size(size_hint: (usize, Option<usize>)) -> usize {
    match size_hint {
        (l, None) => l,
        (_, Some(u)) => u,
    }
}

/// Sets up the given span to be styled as a progress bar.
///
/// The progress bar shows elapsed time, a prefix, the bar itself, the percentage,
/// and a message.
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

/// Sets up the given span to be styled as a sub-task spinner.
///
/// This is useful for indicating progress on a smaller, nested task within a larger operation.
pub fn set_sub_task(span: &Span, msg: &str) {
    span.pb_set_style(
        &ProgressStyle::with_template("  {span_child_prefix} {spinner:.blue} {wide_msg}")
            .unwrap_or(ProgressStyle::default_spinner()),
    );
    span.pb_set_message(msg);
}
