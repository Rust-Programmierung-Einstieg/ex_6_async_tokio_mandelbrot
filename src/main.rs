use std::{
    fs::File,
    io::{Read, Write},
    sync::mpsc::channel,
    time::SystemTime,
};

use num::{
    complex::{Complex64, ComplexFloat},
    Complex,
};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

const CONFIG_FILE_PATH: &str = "config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Grid {
    re_min: f64,
    re_max: f64,
    im_min: f64,
    im_max: f64,
    delta: f64,
}
impl Default for Grid {
    fn default() -> Self {
        Grid {
            re_min: -1.45,
            im_min: -0.9,
            re_max: 0.45,
            im_max: 0.9,
            delta: 0.0005,
        }
    }
}

#[derive(Clone, Debug)]
struct Point {
    position: Complex64,
    value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    grid: Grid,
    iterations: usize,
    bound: f64,
    threads: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            grid: Grid::default(),
            iterations: 200,
            bound: 2.0,
            threads: 1,
        }
    }
}

fn mandelbrot_result(mut point: Point, config: &Config) -> Point {
    let c = point.position;
    let mut z = Complex::new(0.0, 0.0);
    for i in 0..config.iterations {
        z = z * z + c;
        if z.abs() > config.bound {
            break;
        }
        //println!("c: {c}, z_{i}: {z}");
    }
    if z.abs() > config.bound {
        point.value = f64::NAN;
    } else {
        point.value = z.abs();
    }
    point
}

fn write_config_to_file(config: &Config) -> anyhow::Result<()> {
    let text = toml::to_string(config)?;
    let mut file = File::create(CONFIG_FILE_PATH)?;

    file.write_all(text.as_bytes())?;
    Ok(())
}

fn read_config_from_file() -> anyhow::Result<Config> {
    let mut file = File::open(CONFIG_FILE_PATH)?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    toml::from_str(&contents).map_err(anyhow::Error::new)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let start = SystemTime::now();

    let (progress_sender, progress_receiver) = channel::<usize>();
    let config = match read_config_from_file() {
        Ok(config) => config,
        Err(e) => {
            let config = Config::default();
            write_config_to_file(&config)?;
            config
        }
    };

    let mut grid_data: Vec<Point> = vec![];

    // fill grid_data vector

    let mut x: f64 = config.grid.re_min;
    let mut y: f64 = config.grid.im_min;
    while x <= config.grid.re_max {
        x += config.grid.delta;

        while y <= config.grid.im_max {
            y += config.grid.delta;
            //println!("x: {x}, y:{y}");
            let mut point = Point {
                position: Complex::new(x, y),
                value: 0.0,
            };

            grid_data.push(point);
        }

        // reset y coordinate
        y = config.grid.im_min;
    }

    //------------------------------

    let total_work_amount = grid_data.len();
    let mut join_handles: Vec<JoinHandle<Vec<Point>>> = vec![];
    let grid_data_vecs: Vec<Vec<Point>> = grid_data
        .chunks(grid_data.len() / config.threads)
        .map(|s| s.into())
        .collect();

    for v in grid_data_vecs {
        let sender = progress_sender.clone();
        let cfg = config.clone();

        let thread_join_handle = tokio::spawn(async move {
            println!("Thread Started,  work:{}", v.len());
            let mut points_done: Vec<Point> = vec![];
            for point in v {
                points_done.push(mandelbrot_result(point, &cfg));
                if points_done.len() % 1000 == 0 {
                    sender.send(1000).expect("Could send progress");
                }
            }
            sender
                .send(points_done.len() % 1000)
                .expect("Could send progress");
            points_done
        });

        join_handles.push(thread_join_handle);
    }
    // manually drop the last progress sender that we cloned from
    drop(progress_sender);

    let mut total_progress = 0;

    while let Ok(single_progress) = progress_receiver.recv() {
        total_progress += single_progress;
        let progress_percentage = (total_progress as f64 / total_work_amount as f64) * 100_f64;
        print!("\r{progress_percentage:.2}%      ");
    }

    println!("---");
    println!("collecting results");
    let mut points_done_vec: Vec<Point> = Vec::new();

    for join_handle in join_handles {
        let points_done = join_handle.await.expect("could not join thread");
        points_done_vec.extend(points_done);
    }

    println!("Exporting...");

    let filename = "mandelbrot.csv".to_string();
    let mut wtr = csv::Writer::from_path(filename)?;
    wtr.write_record(vec!["re", "im", "value"])?;
    for p in points_done_vec.iter() {
        wtr.serialize((p.position.re(), p.position.im(), p.value))?;
    }
    wtr.flush()?;
    let end = SystemTime::now();
    let duration = end.duration_since(start)?;
    println!("took: {}ms", duration.as_millis());
    Ok(())
}
