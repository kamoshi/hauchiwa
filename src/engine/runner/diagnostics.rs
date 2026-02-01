use std::collections::HashMap;
use std::fmt::{Display, Formatter, Write};

use petgraph::graph::NodeIndex;

use crate::Website;
use crate::engine::TaskExecution;

/// Build diagnostics and performance metrics.
///
/// This struct is returned by [`Website::build`] and contains information about
/// the execution of tasks, such as duration and start times.
#[derive(Debug, Default)]
pub struct Diagnostics {
    /// A map of task node indices to their execution metrics.
    pub execution_times: HashMap<NodeIndex, TaskExecution>,
}

impl Diagnostics {
    /// Renders the task graph as a Mermaid diagram, color-coded by execution duration.
    ///
    /// * **Green**: Fast
    /// * **Yellow**: Moderate
    /// * **Red**: Slow
    /// * **Blue**: Cached (skipped)
    pub fn render_mermaid<G>(&self, site: &Website<G>) -> String
    where
        G: Send + Sync,
    {
        use std::fmt::Write;

        let mut f = String::new();
        writeln!(f, "graph LR").unwrap();

        let times = &self.execution_times;
        let mut min_time = f64::MAX;
        let mut max_time = f64::MIN;

        for t in times.values() {
            let secs = t.duration.as_secs_f64();
            if secs < min_time {
                min_time = secs;
            }
            if secs > max_time {
                max_time = secs;
            }
        }

        if min_time > max_time {
            // No tasks ran
            min_time = 0.0;
            max_time = 0.0;
        }

        // Avoid divide by zero if all tasks took same time
        if (max_time - min_time).abs() < f64::EPSILON {
            max_time = min_time + 1.0;
        }

        for index in site.graph.node_indices() {
            let task = &site.graph[index];
            let name = task.name().replace('"', "\\\""); // Simple escape

            // Determine status and label
            let (label_extra, color_code) = if let Some(exec) = times.get(&index) {
                let duration = exec.duration;
                let duration_str = format!("{:.2?}", duration);

                // Color calculation (Green -> Yellow -> Red)
                let val = duration.as_secs_f64();
                let t = (val - min_time) / (max_time - min_time);

                // t goes 0.0 -> 1.0
                // 0.0 (Green) -> 0.5 (Yellow) -> 1.0 (Red)

                let (r, g, b) = if t < 0.5 {
                    // Green (0, 255, 0) to Yellow (255, 255, 0)
                    // R: 0 -> 255
                    // G: 255
                    // B: 0
                    let t_scaled = t * 2.0; // 0.0 -> 1.0
                    let r = (255.0 * t_scaled) as u8;
                    (r, 255, 0)
                } else {
                    // Yellow (255, 255, 0) to Red (255, 0, 0)
                    // R: 255
                    // G: 255 -> 0
                    // B: 0
                    let t_scaled = (t - 0.5) * 2.0; // 0.0 -> 1.0
                    let g = (255.0 * (1.0 - t_scaled)) as u8;
                    (255, g, 0)
                };

                (duration_str, format!("#{:02X}{:02X}{:02X}", r, g, b))
            } else {
                ("Cached".to_string(), "#ADD8E6".to_string()) // Light Blue
            };

            writeln!(f, "    {:?}[\"{}\\n{}\"]", index.index(), name, label_extra).unwrap();
            writeln!(f, "    style {:?} fill:{}", index.index(), color_code).unwrap();

            if task.is_output() {
                writeln!(f, "    {:?} --> Output", index.index()).unwrap();
            }
        }

        writeln!(f, "    Output[Output]").unwrap();

        for edge in site.graph.edge_indices() {
            let (source, target) = site.graph.edge_endpoints(edge).unwrap();
            let source_task = &site.graph[source];
            let type_name = source_task
                .type_name_output()
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            writeln!(
                f,
                "    {:?} -- \"{}\" --> {:?}",
                source.index(),
                type_name,
                target.index()
            )
            .unwrap();
        }

        f
    }
}

// WATERFALL

struct XmlSafe<'a>(&'a str);

impl<'a> Display for XmlSafe<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for c in self.0.chars() {
            match c {
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '&' => f.write_str("&amp;")?,
                '"' => f.write_str("&quot;")?,
                '\'' => f.write_str("&apos;")?,
                _ => f.write_char(c)?,
            }
        }
        Ok(())
    }
}

// Grouping layout constants so they are easy to tweak in one place.
#[derive(Debug, Clone, Copy)]
struct WaterfallLayout {
    row_height: u32,
    label_width: u32,
    chart_width: u32,
    padding: u32,
    header_height: u32,
    text_space: u32,
}

impl Default for WaterfallLayout {
    fn default() -> Self {
        Self {
            row_height: 30,
            label_width: 300,
            chart_width: 800,
            padding: 10,
            header_height: 30,
            text_space: 80,
        }
    }
}

impl WaterfallLayout {
    fn total_width(&self) -> u32 {
        self.label_width + self.chart_width + (self.padding * 3) + self.text_space
    }

    fn total_height(&self, task_count: usize) -> u32 {
        self.header_height + (task_count as u32 * self.row_height) + self.padding
    }
}

struct TimelineStats {
    global_start: std::time::Instant,
    total_micros: f64,
}

impl TimelineStats {
    fn from_tasks(tasks: &[(NodeIndex, &TaskExecution)]) -> Option<Self> {
        let first = tasks.first()?;
        let global_start = first.1.start;

        let global_end = tasks.iter().map(|(_, t)| t.start + t.duration).max()?;

        let total_duration = global_end.duration_since(global_start);
        // Ensure we never divide by zero
        let total_micros = total_duration.as_micros().max(1) as f64;

        Some(Self {
            global_start,
            total_micros,
        })
    }

    fn format_duration(micros: f64) -> String {
        if micros < 1000.0 {
            format!("{:.0}µs", micros)
        } else {
            format!("{:.2}ms", micros / 1000.0)
        }
    }
}

impl Diagnostics {
    /// Renders a waterfall chart of task execution as an SVG file.
    pub fn render_waterfall_to_file<G>(
        &self,
        site: &Website<G>,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), std::io::Error>
    where
        G: Send + Sync,
    {
        std::fs::write(path, self.render_waterfall(site))
    }

    /// Renders a waterfall chart of task execution as an SVG string.
    pub fn render_waterfall<G>(&self, site: &Website<G>) -> String
    where
        G: Send + Sync,
    {
        // 1. Prepare Data
        let mut ran_tasks: Vec<(NodeIndex, &TaskExecution)> =
            self.execution_times.iter().map(|(k, v)| (*k, v)).collect();

        if ran_tasks.is_empty() {
            return self.render_empty_state();
        }

        ran_tasks.sort_by_key(|(_, t)| t.start);

        // Unwrapping is safe here because we checked is_empty()
        let stats = TimelineStats::from_tasks(&ran_tasks).unwrap();
        let layout = WaterfallLayout::default();

        // 2. Render
        let mut svg = String::with_capacity(ran_tasks.len() * 500); // Pre-allocate approx size

        self.write_svg_header(&mut svg, &layout, ran_tasks.len());
        self.write_grid(&mut svg, &layout, &stats);
        _ = self.write_tasks(&mut svg, &layout, &stats, &ran_tasks, site);

        svg.push_str("</svg>");

        svg
    }

    fn render_empty_state(&self) -> String {
        r#"<svg width="200" height="50" xmlns="http://www.w3.org/2000/svg">
            <text x="10" y="30" font-family="sans-serif">No tasks ran</text>
        </svg>"#
            .to_string()
    }

    fn write_svg_header(&self, buf: &mut String, layout: &WaterfallLayout, task_count: usize) {
        let w = layout.total_width();
        let h = layout.total_height(task_count);

        // CSS extracted for readability
        let css = r#"
        .task-row:nth-child(even) { fill: #f9f9f9; }
        .task-row:nth-child(odd) { fill: #ffffff; }
        text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; font-size: 12px; }
        .bar { fill: #3b82f6; rx: 4; }
        .bar:hover { fill: #2563eb; }
        .label { fill: #333; }
        .time { fill: #666; font-size: 11px; }
        .grid-line { stroke: #e5e7eb; stroke-width: 1; }
        .axis-label { fill: #9ca3af; font-size: 10px; }"#;

        let _ = write!(
            buf,
            r#"<svg width="{}" height="{}" xmlns="http://www.w3.org/2000/svg"><style>{}</style><rect width="100%" height="100%" fill="white" />"#,
            w, h, css
        );
    }

    fn write_grid(&self, buf: &mut String, layout: &WaterfallLayout, stats: &TimelineStats) {
        let steps = 5;
        for i in 0..=steps {
            let pct = i as f64 / steps as f64;
            let current_micros = stats.total_micros * pct;
            let time_label = TimelineStats::format_duration(current_micros);

            let x = layout.label_width as f64
                + layout.padding as f64
                + (layout.chart_width as f64 * pct);

            // Note: SVG ignores lines drawn past the viewbox height, so we can be lazy with y2 or pass total height
            let _ = write!(
                buf,
                r#"<line x1="{x:.1}" y1="{y1}" x2="{x:.1}" y2="100%" class="grid-line" /><text x="{x:.1}" y="{y_text}" text-anchor="middle" class="axis-label">{label}</text>"#,
                x = x,
                y1 = layout.header_height,
                y_text = layout.header_height - 5,
                label = time_label
            );
        }
    }

    fn write_tasks<G>(
        &self,
        buf: &mut String,
        layout: &WaterfallLayout,
        stats: &TimelineStats,
        tasks: &[(NodeIndex, &TaskExecution)],
        site: &Website<G>,
    ) -> std::fmt::Result
    where
        G: Send + Sync,
    {
        for (i, (node_idx, exec)) in tasks.iter().enumerate() {
            let task = &site.graph[*node_idx];
            let raw_name = task.name();
            let safe_name = XmlSafe(&raw_name); // Validates XML safety

            let y_pos = layout.header_height + (i as u32 * layout.row_height);
            let y_center = y_pos + (layout.row_height / 2);

            // Background Row
            write!(
                buf,
                r#"<rect x="0" y="{}" width="100%" height="{}" class="task-row" />"#,
                y_pos, layout.row_height
            )?;

            // Label
            write!(
                buf,
                r#"<text x="{}" y="{}" class="label" dominant-baseline="middle">{}</text>"#,
                layout.padding, y_center, safe_name
            )?;

            // Bar Math
            let offset_micros = exec.start.duration_since(stats.global_start).as_micros() as f64;
            let duration_micros = exec.duration.as_micros() as f64;

            let bar_x = layout.label_width as f64
                + layout.padding as f64
                + (offset_micros / stats.total_micros * layout.chart_width as f64);

            let bar_w = (duration_micros / stats.total_micros * layout.chart_width as f64).max(1.0);

            // Draw Bar
            write!(
                buf,
                r#"<rect x="{x:.1}" y="{y}" width="{w:.1}" height="{h}" class="bar"><title>{name}: {dur:.2?}</title></rect>"#,
                x = bar_x,
                y = y_pos + 5,
                w = bar_w,
                h = layout.row_height - 10,
                name = safe_name,
                dur = exec.duration
            )?;

            // Duration Label
            let dur_text = TimelineStats::format_duration(duration_micros);
            write!(
                buf,
                r#"<text x="{x:.1}" y="{y}" class="time" dominant-baseline="middle">{text}</text>"#,
                x = bar_x + bar_w + 5.0,
                y = y_center,
                text = dur_text
            )?;
        }

        Ok(())
    }
}
