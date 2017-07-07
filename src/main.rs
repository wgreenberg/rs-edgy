use std::thread;

extern crate gst;
extern crate rscam;

use rscam::{Camera, Config, Frame};
const HEIGHT: usize = 480;
const WIDTH: usize = 640;
const BUF_SIZE: usize = HEIGHT * WIDTH * 2;

struct Neighbors<'a> {
    frame: &'a Frame,
    idx: i64,
}
impl<'a> Iterator for Neighbors<'a> {
    type Item = [u8; 9];
    fn next (&mut self) -> Option<[u8; 9]> {
        if self.idx == (HEIGHT * WIDTH) as i64 {
            None
        } else {
            let x = self.idx % (WIDTH as i64);
            let y = self.idx / (WIDTH as i64);
            self.idx += 1;
            Some([
                get_pix((x-1, y-1), self.frame),
                get_pix((x, y-1), self.frame),
                get_pix((x+1, y-1), self.frame),
                get_pix((x-1, y), self.frame),
                get_pix((x, y), self.frame),
                get_pix((x+1, y), self.frame),
                get_pix((x-1, y+1), self.frame),
                get_pix((x, y+1), self.frame),
                get_pix((x+1, y+1), self.frame),
            ])
        }
    }
}

fn convolve (frame: & Frame) -> Neighbors {
    Neighbors { frame: frame, idx: 0 }
}

fn get_pix ((x, y): (i64, i64), frame: &Frame) -> u8 {
    if x < 0 || y < 0 || x >= WIDTH as i64 || y >= HEIGHT as i64 {
        0
    } else {
        let idx = ((x + y * (WIDTH as i64)) / 2) as usize;
        let y0 = frame[idx];
        let y1 = frame[idx + 2];
        if x % 2 == 0 { y0 } else { y1 }
    }
}

fn filter (input: &Frame, output: &mut [u8]) {
    let mut i = 0;
    for neighbors in convolve(input) {
        output[i+0] = neighbors[4];
        output[i+1] = 0x80;
        i += 2;
    }
}

fn main() {
    // GStreamer setup
    gst::init();
    let mut pipeline = gst::Pipeline::new_from_str("appsrc caps=\"video/x-raw,format=YUY2,width=640,height=480,framerate=1/30\" name=appsrc0 ! videoconvert ! autovideosink").unwrap();
    let mut mainloop = gst::MainLoop::new();

    // rscam setup
    let mut camera = Camera::new("/dev/video0").unwrap();
    camera.start(&Config {
        interval: (1, 30),
        resolution: (640, 480),
        ..Default::default()
    }).unwrap();

    let appsrc = pipeline.get_by_name("appsrc0").expect("Couldn't get appsrc from pipeline");
    let mut appsrc = gst::AppSrc::new_from_element(appsrc);
    let mut bufferpool = gst::BufferPool::new().unwrap();
    let appsrc_caps = appsrc.caps().unwrap();
    bufferpool.set_params(&appsrc_caps,640*480*2,0,0);
    if bufferpool.set_active(true).is_err(){
        panic!("Couldn't activate buffer pool");
    }

    let mut bus = pipeline.bus().expect("Couldn't get bus from pipeline");
    let bus_receiver = bus.receiver();

    mainloop.spawn();
    pipeline.play();

    thread::spawn(move|| {
        let mut cv_buf: [u8; BUF_SIZE] = [0; BUF_SIZE];
        loop {
            let frame = camera.capture().unwrap();
            // frame not fully captured???
            if frame.len() != BUF_SIZE {
                continue;
            }
            filter(&frame, &mut cv_buf);
            if let Some(mut buffer) = bufferpool.acquire_buffer() {
                let mut i = 0;
                // copy the webcam frame to the appsrc... is there a better way
                // to do this???
                buffer.map_write(|mut mapping| {
                    for c in mapping.iter_mut::<u8>() {
                        *c = cv_buf[i];
                        i += 1;
                    }
                }).ok();
                appsrc.push_buffer(buffer);
            } else {
                println!("Couldn't get buffer, sending EOS and finishing thread");
                appsrc.end_of_stream();
                break;
            }
        }
    });

    for message in bus_receiver.iter(){
        match message.parse(){
            gst::Message::StateChangedParsed{ref old, ref new, ..} => {
                println!("element `{}` changed from {:?} to {:?}", message.src_name(), old, new);
            }
            gst::Message::ErrorParsed{ref error, ref debug, ..} => {
                println!("error msg from element `{}`: {}, {}. Quitting", message.src_name(), error.message(), debug);
                break;
            }
            gst::Message::Eos(_) => {
                println!("eos received quiting");
                break;
            }
            _ => {
                println!("msg of type `{}` from element `{}`", message.type_name(), message.src_name());
            }
        }
    }

    mainloop.quit();
}
