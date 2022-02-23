// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility dashboard`
//!
//! Provides a captive dashboard that graphs sensor values over time.  (The
//! `sensor` task is required for operation; see the documentation for
//! `humility sensors` for more details.)
//!
//! If `-o` is provided, it specifies an output file for any raw sensor data
//! graphed by the dashboard.
//!

use anyhow::{bail, Result};
use clap::Command as ClapCommand;
use clap::{CommandFactory, Parser};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use hif::*;
use humility::core::Core;
use humility::hubris::*;
use humility_cmd::hiffy::*;
use humility_cmd::idol;
use humility_cmd::{Archive, Args, Attach, Command, Validate};
use std::fs::File;
use std::io;
use std::io::Write;
use std::time::{Duration, Instant};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Span, Spans},
    widgets::{
        Axis, Block, Borders, Chart, Dataset, List, ListItem, ListState,
    },
    Frame, Terminal,
};

#[derive(Parser, Debug)]
#[clap(name = "dashboard", about = env!("CARGO_PKG_DESCRIPTION"))]
struct DashboardArgs {
    /// sets timeout
    #[clap(
        long, short = 'T', default_value = "5000", value_name = "timeout_ms",
        parse(try_from_str = parse_int::parse)
    )]
    timeout: u32,

    /// CSV output file
    #[clap(long, short)]
    output: Option<String>,
}

struct StatefulList {
    state: ListState,
    n: usize,
}

impl StatefulList {
    fn next(&mut self) {
        self.state.select(match self.state.selected() {
            Some(ndx) => Some((ndx + 1) % self.n),
            None => Some(0),
        });
    }

    fn previous(&mut self) {
        self.state.select(match self.state.selected() {
            Some(ndx) if ndx == 0 => Some(self.n - 1),
            Some(ndx) => Some(ndx - 1),
            None => Some(0),
        });
    }

    fn unselect(&mut self) {
        self.state.select(None);
    }
}

struct Series {
    name: String,
    color: Color,
    data: Vec<(f64, f64)>,
    raw: Vec<Option<f32>>,
}

trait Attributes {
    fn label(&self) -> String;
    fn legend_label(&self) -> String;
    fn x_axis_label(&self) -> String {
        "Time".to_string()
    }
    fn y_axis_label(&self) -> String;
    fn axis_value(&self, val: f64) -> String;
    fn legend_value(&self, val: f64) -> String;

    fn increase(&mut self, _ndx: usize) -> Option<u8> {
        None
    }

    fn decrease(&mut self, _ndx: usize) -> Option<u8> {
        None
    }

    fn clear(&mut self) {}
}

struct TempGraph;

impl Attributes for TempGraph {
    fn label(&self) -> String {
        "Temperature".to_string()
    }
    fn legend_label(&self) -> String {
        "Sensors".to_string()
    }

    fn y_axis_label(&self) -> String {
        "Degrees Celsius".to_string()
    }

    fn axis_value(&self, val: f64) -> String {
        format!("{:2.0}°", val)
    }

    fn legend_value(&self, val: f64) -> String {
        format!("{:4.2}°", val)
    }
}

struct FanGraph(Vec<u8>);

impl FanGraph {
    fn new(len: usize) -> Self {
        let v = vec![0; len];
        FanGraph(v)
    }
}

impl Attributes for FanGraph {
    fn label(&self) -> String {
        "Fan speed".to_string()
    }
    fn legend_label(&self) -> String {
        "Fans".to_string()
    }

    fn y_axis_label(&self) -> String {
        "RPM".to_string()
    }

    fn axis_value(&self, val: f64) -> String {
        format!("{:3.1}K", val / 1000.0)
    }

    fn legend_value(&self, val: f64) -> String {
        format!("{:.0}", val)
    }

    fn increase(&mut self, ndx: usize) -> Option<u8> {
        let current = self.0[ndx];
        let nval = current + 20;

        self.0[ndx] = if nval <= 100 { nval } else { 100 };
        Some(self.0[ndx])
    }

    fn decrease(&mut self, ndx: usize) -> Option<u8> {
        let current = self.0[ndx];
        let nval = current as i8 - 20;

        self.0[ndx] = if nval >= 0 { nval as u8 } else { 0 };
        Some(self.0[ndx])
    }

    fn clear(&mut self) {
        for val in self.0.iter_mut() {
            *val = 0;
        }
    }
}

struct CurrentGraph;

impl Attributes for CurrentGraph {
    fn label(&self) -> String {
        "Output current".to_string()
    }
    fn legend_label(&self) -> String {
        "Regulators".to_string()
    }

    fn y_axis_label(&self) -> String {
        "Amperes".to_string()
    }

    fn axis_value(&self, val: f64) -> String {
        format!("{:2.2}A", val)
    }

    fn legend_value(&self, val: f64) -> String {
        format!("{:3.2}A", val)
    }
}

struct Graph {
    series: Vec<Series>,
    legend: StatefulList,
    time: usize,
    width: usize,
    interpolate: usize,
    bounds: [f64; 2],
    attributes: Box<dyn Attributes>,
}

impl Graph {
    fn new(all: &[String], attr: Box<dyn Attributes>) -> Result<Self> {
        let mut series = vec![];

        let colors = [
            Color::Yellow,
            Color::Green,
            Color::Magenta,
            Color::White,
            Color::Red,
            Color::LightRed,
            Color::Blue,
            Color::LightMagenta,
            Color::LightYellow,
            Color::LightCyan,
            Color::LightGreen,
            Color::LightBlue,
            Color::LightRed,
        ];

        for (ndx, s) in all.iter().enumerate() {
            series.push(Series {
                name: s.to_string(),
                color: colors[ndx % colors.len()],
                data: Vec::new(),
                raw: Vec::new(),
            })
        }

        Ok(Graph {
            series,
            legend: StatefulList { state: ListState::default(), n: all.len() },
            time: 0,
            width: 600,
            interpolate: 0,
            bounds: [20.0, 120.0],
            attributes: attr,
        })
    }

    fn data(&mut self, data: &[Option<f32>]) {
        for (ndx, s) in self.series.iter_mut().enumerate() {
            s.raw.push(data[ndx]);
        }

        self.time += 1;
    }

    fn update_data(&mut self) {
        for s in &mut self.series {
            s.data = Vec::new();
        }

        for i in 0..self.width {
            if self.time < self.width - i {
                continue;
            }

            let offs = (self.time - (self.width - i)) as usize;

            for (_ndx, s) in &mut self.series.iter_mut().enumerate() {
                if let Some(datum) = s.raw[offs] {
                    let point = (i as f64, datum as f64);

                    if self.interpolate != 0 {
                        if let Some(last) = s.data.last() {
                            let x_delta = point.0 - last.0;
                            let slope = (point.1 - last.1) / x_delta;
                            let x_inc = x_delta / self.interpolate as f64;

                            for x in 0..self.interpolate {
                                s.data.push((
                                    point.0 + x as f64 * x_inc,
                                    point.1 + (slope * x_inc),
                                ));
                            }
                        }
                    }

                    s.data.push((i as f64, datum as f64));
                }
            }
        }

        self.update_bounds();
    }

    fn update_bounds(&mut self) {
        let selected = self.legend.state.selected();
        let mut min = None;
        let mut max = None;

        for (ndx, s) in self.series.iter().enumerate() {
            if let Some(selected) = selected {
                if ndx != selected {
                    continue;
                }
            }

            for (_, datum) in &s.data {
                min = match min {
                    Some(min) if datum < min => Some(datum),
                    None => Some(datum),
                    _ => min,
                };

                max = match max {
                    Some(max) if datum > max => Some(datum),
                    None => Some(datum),
                    _ => max,
                };
            }
        }

        if let Some(min) = min {
            self.bounds[0] = ((min * 0.85) / 2.0) * 2.0;
        }

        if self.bounds[0] < 0.0 {
            self.bounds[0] = 0.0;
        }

        if let Some(max) = max {
            self.bounds[1] = ((max * 1.15) / 2.0) * 2.0;
        }
    }

    fn previous(&mut self) {
        self.legend.previous();
    }

    fn next(&mut self) {
        self.legend.next();
    }

    fn unselect(&mut self) {
        self.legend.unselect();
    }

    fn set_interpolate(&mut self) {
        let interpolate = (1000.0 - self.width as f64) / self.width as f64;

        if interpolate >= 1.0 {
            self.interpolate = interpolate as usize;
        } else {
            self.interpolate = 0;
        }
    }

    fn zoom_in(&mut self) {
        self.width = (self.width as f64 * 0.8) as usize;
        self.set_interpolate();
    }

    fn zoom_out(&mut self) {
        self.width = (self.width as f64 * 1.25) as usize;
        self.set_interpolate();
    }
}

struct Dashboard<'a> {
    hubris: &'a HubrisArchive,
    context: HiffyContext<'a>,
    ops: Vec<Op>,
    graphs: Vec<Graph>,
    current: usize,
    work: Vec<Vec<Op>>,
    last: Instant,
    interval: u32,
    outstanding: bool,
    output: Option<File>,
}

impl<'a> Dashboard<'a> {
    fn new(
        hubris: &'a HubrisArchive,
        core: &mut dyn Core,
        subargs: &DashboardArgs,
    ) -> Result<Dashboard<'a>> {
        let mut context = HiffyContext::new(hubris, core, subargs.timeout)?;
        let mut ops = vec![];

        let temps = sensor_ops(hubris, &mut context, &mut ops, |s| {
            s.kind == HubrisSensorKind::Temperature
        })?;

        let fans = sensor_ops(hubris, &mut context, &mut ops, |s| {
            s.kind == HubrisSensorKind::Speed
        })?;

        let current = sensor_ops(hubris, &mut context, &mut ops, |s| {
            s.kind == HubrisSensorKind::Current
        })?;

        ops.push(Op::Done);

        context.start(core, ops.as_slice(), None)?;

        let output = if let Some(output) = &subargs.output {
            let mut f = File::create(output)?;
            writeln!(&mut f, "{}", temps.join(","))?;
            Some(f)
        } else {
            None
        };

        let graphs = vec![
            Graph::new(&temps, Box::new(TempGraph))?,
            Graph::new(&fans, Box::new(FanGraph::new(fans.len())))?,
            Graph::new(&current, Box::new(CurrentGraph))?,
        ];

        Ok(Dashboard {
            hubris,
            context,
            ops,
            graphs,
            current: 0,
            outstanding: true,
            last: Instant::now(),
            interval: 1000,
            work: Vec::new(),
            output,
        })
    }

    fn dequeue_work(&mut self, core: &mut dyn Core) -> Result<()> {
        for w in &self.work {
            let _results = self.context.run(core, w.as_slice(), None)?;
        }

        self.work = vec![];
        Ok(())
    }

    fn enqueue_work(
        &mut self,
        core: &mut dyn Core,
        ops: Vec<Op>,
    ) -> Result<()> {
        if self.outstanding {
            self.work.push(ops);
            Ok(())
        } else {
            let _results = self.context.run(core, ops.as_slice(), None)?;
            Ok(())
        }
    }

    fn need_update(&mut self, core: &mut dyn Core) -> Result<bool> {
        if self.outstanding {
            if self.context.done(core)? {
                let results = self.context.results(core)?;
                let mut raw = vec![];

                for r in &results {
                    raw.push(if let Ok(val) = r {
                        Some(f32::from_le_bytes(val[0..4].try_into()?))
                    } else {
                        None
                    });
                }

                let mut offs = 0;

                for graph in self.graphs.iter_mut() {
                    graph.data(&raw[offs..]);
                    offs += graph.series.len();
                }

                if let Some(output) = &mut self.output {
                    for val in raw {
                        if let Some(val) = val {
                            write!(output, "{:.2},", val)?;
                        } else {
                            write!(output, ",")?;
                        }
                    }
                    writeln!(output)?;
                }

                self.outstanding = false;
                self.dequeue_work(core)?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            if self.last.elapsed().as_millis() > self.interval.into() {
                self.context.start(core, self.ops.as_slice(), None)?;
                self.last = Instant::now();
                self.outstanding = true;
            }

            Ok(false)
        }
    }

    fn update_data(&mut self) {
        for graph in self.graphs.iter_mut() {
            graph.update_data();
        }
    }

    fn up(&mut self) {
        self.graphs[self.current].previous();
    }

    fn down(&mut self) {
        self.graphs[self.current].next();
    }

    fn esc(&mut self) {
        self.graphs[self.current].unselect();
    }

    fn tab(&mut self) {
        self.current = (self.current + 1) % self.graphs.len();
    }

    fn increase(&mut self, core: &mut dyn Core) {
        let graph = &mut self.graphs[self.current];

        if let Some(selected) = graph.legend.state.selected() {
            if let Some(pwm) = graph.attributes.increase(selected) {
                self.fan_to(core, selected, pwm).unwrap();
            }
        }
    }

    fn decrease(&mut self, core: &mut dyn Core) {
        let graph = &mut self.graphs[self.current];

        if let Some(selected) = graph.legend.state.selected() {
            if let Some(pwm) = graph.attributes.decrease(selected) {
                self.fan_to(core, selected, pwm).unwrap();
            }
        }
    }

    fn enter(&mut self) {}

    fn set_a0(&mut self, core: &mut dyn Core) -> Result<()> {
        let ops = power_ops(self.hubris, &mut self.context, "A0")?;
        self.enqueue_work(core, ops)?;
        Ok(())
    }

    fn set_a2(&mut self, core: &mut dyn Core) -> Result<()> {
        let ops = power_ops(self.hubris, &mut self.context, "A2")?;
        self.enqueue_work(core, ops)?;
        Ok(())
    }

    fn fans_on(&mut self, core: &mut dyn Core) -> Result<()> {
        let ops = fan_ops(self.hubris, &mut self.context, true)?;
        self.enqueue_work(core, ops)?;
        Ok(())
    }

    fn fans_off(&mut self, core: &mut dyn Core) -> Result<()> {
        let ops = fan_ops(self.hubris, &mut self.context, false)?;
        self.enqueue_work(core, ops)?;
        Ok(())
    }

    fn fan_to(
        &mut self,
        core: &mut dyn Core,
        index: usize,
        pwm: u8,
    ) -> Result<()> {
        let ops = pwm_ops(self.hubris, &mut self.context, index, pwm)?;
        self.enqueue_work(core, ops)?;
        Ok(())
    }

    fn zoom_in(&mut self) {
        for graph in self.graphs.iter_mut() {
            graph.zoom_in();
        }
    }

    fn zoom_out(&mut self) {
        for graph in self.graphs.iter_mut() {
            graph.zoom_out();
        }
    }
}

fn run_dashboard<B: Backend>(
    terminal: &mut Terminal<B>,
    mut dashboard: Dashboard,
    core: &mut dyn Core,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(100);

    loop {
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        let update = if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('2') => dashboard.set_a2(core)?,
                    KeyCode::Char('0') => dashboard.set_a0(core)?,
                    KeyCode::Char('F') => dashboard.fans_on(core)?,
                    KeyCode::Char('f') => dashboard.fans_off(core)?,
                    KeyCode::Char('+') => dashboard.zoom_in(),
                    KeyCode::Char('-') => dashboard.zoom_out(),
                    KeyCode::Char('>') => dashboard.increase(core),
                    KeyCode::Char('<') => dashboard.decrease(core),
                    KeyCode::Up => dashboard.up(),
                    KeyCode::Down => dashboard.down(),
                    KeyCode::Esc => dashboard.esc(),
                    KeyCode::Tab => dashboard.tab(),
                    KeyCode::Enter => dashboard.enter(),
                    _ => {}
                }
            }
            true
        } else {
            dashboard.need_update(core)?
        };

        if update {
            dashboard.update_data();
            terminal.draw(|f| draw(f, &mut dashboard))?;
        }

        last_tick = Instant::now();
    }
}

fn dashboard(
    hubris: &HubrisArchive,
    core: &mut dyn Core,
    _args: &Args,
    subargs: &[String],
) -> Result<()> {
    let subargs = DashboardArgs::try_parse_from(subargs)?;
    let dashboard = Dashboard::new(hubris, core, &subargs)?;

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_dashboard(&mut terminal, dashboard, core);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res?;

    Ok(())
}

pub fn init() -> (Command, ClapCommand<'static>) {
    (
        Command::Attached {
            name: "dashboard",
            archive: Archive::Required,
            attach: Attach::LiveOnly,
            validate: Validate::Booted,
            run: dashboard,
        },
        DashboardArgs::command(),
    )
}

fn sensor_ops(
    hubris: &HubrisArchive,
    context: &mut HiffyContext,
    ops: &mut Vec<Op>,
    capture: impl Fn(&HubrisSensor) -> bool,
) -> Result<Vec<String>> {
    let mut sensors = vec![];
    let funcs = context.functions()?;
    let op = idol::IdolOperation::new(hubris, "Sensor", "get", None)?;

    let ok = hubris.lookup_basetype(op.ok)?;

    if ok.encoding != HubrisEncoding::Float {
        bail!("expected return value of read_sensor() to be a float");
    }

    if ok.size != 4 {
        bail!("expected return value of read_sensor() to be an f32");
    }

    for (i, s) in hubris.manifest.sensors.iter().enumerate() {
        if !capture(s) {
            continue;
        }

        let payload =
            op.payload(&[("id", idol::IdolArgument::Scalar(i as u64))])?;
        context.idol_call_ops(&funcs, &op, &payload, ops)?;
        sensors.push(s.name.clone());
    }

    Ok(sensors)
}

fn power_ops(
    hubris: &HubrisArchive,
    context: &mut HiffyContext,
    state: &str,
) -> Result<Vec<Op>> {
    let mut ops = vec![];
    let funcs = context.functions()?;
    let op = idol::IdolOperation::new(hubris, "Sequencer", "set_state", None)?;

    let payload =
        op.payload(&[("state", idol::IdolArgument::String(state))])?;
    context.idol_call_ops(&funcs, &op, &payload, &mut ops)?;
    ops.push(Op::Done);

    Ok(ops)
}

fn fan_ops(
    hubris: &HubrisArchive,
    context: &mut HiffyContext,
    on: bool,
) -> Result<Vec<Op>> {
    let mut ops = vec![];
    let funcs = context.functions()?;
    let op = idol::IdolOperation::new(
        hubris,
        "Sequencer",
        if on { "fans_on" } else { "fans_off" },
        None,
    )?;

    let payload = vec![];
    context.idol_call_ops(&funcs, &op, &payload, &mut ops)?;
    ops.push(Op::Done);

    Ok(ops)
}

fn pwm_ops(
    hubris: &HubrisArchive,
    context: &mut HiffyContext,
    index: usize,
    pwm: u8,
) -> Result<Vec<Op>> {
    let mut ops = vec![];
    let funcs = context.functions()?;
    let op = idol::IdolOperation::new(hubris, "Thermal", "set_fan_pwm", None)?;

    let payload = op.payload(&[
        ("index", idol::IdolArgument::Scalar(index as u64)),
        ("pwm", idol::IdolArgument::Scalar(pwm as u64)),
    ])?;

    context.idol_call_ops(&funcs, &op, &payload, &mut ops)?;
    ops.push(Op::Done);

    Ok(ops)
}

fn draw_graph<B: Backend>(f: &mut Frame<B>, parent: Rect, graph: &mut Graph) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [Constraint::Ratio(4, 5), Constraint::Ratio(1, 5)].as_ref(),
        )
        .split(parent);

    let x_labels = vec![
        Span::styled(
            format!("t-{}", graph.width),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("t-{}", 1),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];

    let mut datasets = vec![];
    let selected = graph.legend.state.selected();

    for (ndx, s) in graph.series.iter().enumerate() {
        if let Some(selected) = selected {
            if ndx != selected {
                continue;
            }
        }

        datasets.push(
            Dataset::default()
                .name(&s.name)
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(s.color))
                .data(&s.data),
        );
    }

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(Span::styled(
                    graph.attributes.label(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL),
        )
        .x_axis(
            Axis::default()
                .title(graph.attributes.x_axis_label())
                .style(Style::default().fg(Color::Gray))
                .labels(x_labels)
                .bounds([0.0, graph.width as f64]),
        )
        .y_axis(
            Axis::default()
                .title(graph.attributes.y_axis_label())
                .style(Style::default().fg(Color::Gray))
                .labels(vec![
                    Span::styled(
                        graph.attributes.axis_value(graph.bounds[0]),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        graph.attributes.axis_value(graph.bounds[1]),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ])
                .bounds(graph.bounds),
        );

    f.render_widget(chart, chunks[0]);

    let mut rows = vec![];

    for s in &graph.series {
        let val = match s.raw.last() {
            None | Some(None) => "-".to_string(),
            Some(Some(val)) => graph.attributes.legend_value((*val).into()),
        };

        rows.push(ListItem::new(Spans::from(vec![
            Span::styled(
                format!("{:<15}", s.name),
                Style::default().fg(s.color),
            ),
            Span::styled(val, Style::default().fg(s.color)),
        ])));
    }

    let list = List::new(rows)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(graph.attributes.legend_label()),
        )
        .highlight_style(
            Style::default()
                .bg(Color::LightGreen)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    // We can now render the item list
    f.render_stateful_widget(list, chunks[1], &mut graph.legend.state);
}

fn draw<B: Backend>(f: &mut Frame<B>, dashboard: &mut Dashboard) {
    let size = f.size();
    let screen = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Ratio(1, 2),
                Constraint::Ratio(1, 4),
                Constraint::Ratio(1, 4),
            ]
            .as_ref(),
        )
        .split(size);

    draw_graph(f, screen[0], &mut dashboard.graphs[0]);
    draw_graph(f, screen[1], &mut dashboard.graphs[1]);
    draw_graph(f, screen[2], &mut dashboard.graphs[2]);
}
