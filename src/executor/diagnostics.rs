use std::collections::HashMap;

use petgraph::graph::NodeIndex;

use crate::Website;
use crate::executor::TaskExecution;

#[derive(Debug, Default)]
pub struct Diagnostics {
    pub execution_times: HashMap<NodeIndex, TaskExecution>,
}

impl Diagnostics {
    pub fn render_waterfall<G>(&self, site: &Website<G>) -> String
    where
        G: Send + Sync,
    {
        use std::fmt::Write;

        let mut output = String::new();

        // 1. Collect and sort ran tasks
        let mut ran_tasks: Vec<(NodeIndex, &TaskExecution)> =
            self.execution_times.iter().map(|(k, v)| (*k, v)).collect();

        if ran_tasks.is_empty() {
            return "<svg width=\"200\" height=\"50\" xmlns=\"http://www.w3.org/2000/svg\"><text x=\"10\" y=\"30\" font-family=\"sans-serif\">No tasks ran</text></svg>".to_string();
        }

        // Sort by start time
        ran_tasks.sort_by_key(|(_, t)| t.start);

        // 2. Determine global timeline
        let global_start = ran_tasks.first().unwrap().1.start;
        let global_end = ran_tasks
            .iter()
            .map(|(_, t)| t.start + t.duration)
            .max()
            .unwrap();

        let total_duration = global_end.duration_since(global_start);
        let total_micros = total_duration.as_micros().max(1) as f64;

        // 3. Layout constants
        let row_height = 30;
        let label_width = 300;
        let chart_width = 800;
        let padding = 10;
        let header_height = 30;
        let text_space = 80;

        let width = label_width + chart_width + (padding * 3) + text_space;
        let height = header_height + (ran_tasks.len() as u32 * row_height) + padding;

        // 4. Generate SVG Header
        write!(output, r#"<svg width="{}" height="{}" xmlns="http://www.w3.org/2000/svg">
    <style>
        .task-row:nth-child(even) {{ fill: #f9f9f9; }}
        .task-row:nth-child(odd) {{ fill: #ffffff; }}
        text {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; font-size: 12px; }}
        .bar {{ fill: #3b82f6; rx: 4; }}
        .bar:hover {{ fill: #2563eb; }}
        .label {{ fill: #333; }}
        .time {{ fill: #666; font-size: 11px; }}
        .grid-line {{ stroke: #e5e7eb; stroke-width: 1; }}
        .axis-label {{ fill: #9ca3af; font-size: 10px; }}
    </style>
    <rect width="100%" height="100%" fill="white" />
"#, width, height).unwrap();

        // 5. Draw Grid/Axis (Optional but nice)
        // Draw 5 vertical grid lines
        for i in 0..=5 {
            let pct = i as f64 / 5.0;

            let current_micros = total_micros * pct;
            let time_label = if total_micros < 1000.0 {
                // If total chart is small, show microseconds
                format!("{:.0}µs", current_micros)
            } else {
                // Otherwise show fractional milliseconds
                format!("{:.2}ms", current_micros / 1000.0)
            };

            let x = label_width as f64 + padding as f64 + (chart_width as f64 * pct);

            write!(output, r#"    <line x1="{x}" y1="{header_height}" x2="{x}" y2="{height}" class="grid-line" />
    <text x="{x}" y="{y}" text-anchor="middle" class="axis-label">{text}</text>
"#, x=x, y=header_height - 5, text=time_label, header_height=header_height, height=height).unwrap();
        }

        // 6. Draw Tasks
        for (i, (node_idx, exec)) in ran_tasks.iter().enumerate() {
            let task = &site.graph[*node_idx];
            let name = task.get_name();

            let y_pos = header_height + (i as u32 * row_height);

            // Background row
            write!(
                output,
                r#"    <rect x="0" y="{y}" width="{w}" height="{h}" class="task-row" />"#,
                y = y_pos,
                w = width,
                h = row_height
            )
            .unwrap();

            // Task Label
            // Truncate if too long?
            write!(output, r#"    <text x="{x}" y="{y}" class="label" dominant-baseline="middle">{name}</text>"#,
                x=padding, y=y_pos + (row_height/2), name=name).unwrap();

            // Bar calculation
            let offset_micros = exec.start.duration_since(global_start).as_micros() as f64;
            let duration_micros = exec.duration.as_micros() as f64;

            let bar_start_x = label_width as f64
                + padding as f64
                + (offset_micros / total_micros * chart_width as f64);

            let bar_width = (duration_micros / total_micros * chart_width as f64).max(1.0); // Min 1px

            // Draw Bar
            write!(
                output,
                r#"    <rect x="{x}" y="{y}" width="{w}" height="{h}" class="bar">
        <title>{name}: {dur:.2?}</title>
    </rect>"#,
                x = bar_start_x,
                y = y_pos + 5, // padding within row
                w = bar_width,
                h = row_height - 10,
                name = name,
                dur = exec.duration
            )
            .unwrap();

            let text_label = if duration_micros < 1000.0 {
                format!("{:.0}µs", duration_micros)
            } else {
                format!("{:.2}ms", duration_micros / 1000.0)
            };

            // Duration Label (right of bar if space permits, otherwise omit or put inside?)
            // Putting it simply next to the bar
            write!(
                output,
                r#"    <text x="{x}" y="{y}" class="time" dominant-baseline="middle">{lbl}</text>"#,
                x = bar_start_x + bar_width + 5.0,
                y = y_pos + (row_height / 2),
                lbl = text_label
            )
            .unwrap();
        }

        output.push_str("</svg>");
        output
    }

    pub fn render_waterfall_to_file<G>(
        &self,
        site: &Website<G>,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), std::io::Error>
    where
        G: Send + Sync,
    {
        let svg = self.render_waterfall(site);
        std::fs::write(path, svg)
    }

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
            let name = task.get_name().replace('"', "\\\""); // Simple escape

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
                .get_output_type_name()
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
