#![no_main]
#![no_std]
// to use the global allocator
#![feature(alloc_error_handler)]

mod allocator;
mod clock;

use core::{
    alloc::Layout,
    borrow::Borrow,
    mem::MaybeUninit,
    num::{NonZeroU16, NonZeroU8},
    ptr::NonNull,
};

use allocator::MyAllocator;
use clock::StmClock;
use panic_halt as _;

use alloc_cortex_m::CortexMHeap;
use cortex_m_rt::entry;

use cortex_m as _;

use embedded_time::{duration::Milliseconds, Clock};
use hal::{
    delay::{DelayFromCountDownTimer, SYSTDelayExt},
    fdcan::{
        config::{ClockDivider, NominalBitTiming},
        filter::{StandardFilter, StandardFilterSlot},
        FdCan,
    },
    gpio::{GpioExt, Speed},
    prelude::*,
    rcc::{Config, PLLSrc, PllConfig, Rcc, RccExt, SysClockSrc},
    stm32::Peripherals,
    timer::{MonoTimer, Timer},
};
use rlsf::Tlsf;
// use log::info;
use stm32g4xx_hal as hal;

use uavcan::{
    session::HeapSessionManager,
    transfer::Transfer,
    transport::can::{Can, CanMetadata},
    Node, Priority, Subscription, TransferKind,
};
// use util::logger;

static mut POOL: MaybeUninit<[u8; 1024]> = MaybeUninit::uninit();

#[global_allocator]
static ALLOCATOR: MyAllocator = MyAllocator::INIT;

#[entry]
fn main() -> ! {
    // Initialize cortex heap allocator
    // let start = cortex_m_rt::heap_start() as usize;
    // let size = 1024; // in bytes

    let cursor = unsafe { POOL.as_mut_ptr() } as *mut u8;
    let size = 1024;
    unsafe { ALLOCATOR.init(cursor, size) };

    // logger::init();

    // define peripherals of the board
    let dp = Peripherals::take().unwrap();
    let cp = cortex_m::Peripherals::take().expect("cannot take core peripherals");
    let rcc = dp.RCC.constrain();
    let mut rcc = config_rcc(rcc);

    let gpioa = dp.GPIOA.split(&mut rcc);

    let mut led = gpioa.pa5.into_push_pull_output();
    let mut delay_syst = cp.SYST.delay(&rcc.clocks);
    // init can
    let can = {
        let rx = gpioa.pa11.into_alternate().set_speed(Speed::VeryHigh);
        let tx = gpioa.pa12.into_alternate().set_speed(Speed::VeryHigh);

        let can = FdCan::new_with_clock_source(
            dp.FDCAN1,
            tx,
            rx,
            &rcc,
            hal::fdcan::FdCanClockSource::PCLK,
        );

        let mut can = can.into_config_mode();
        can.set_protocol_exception_handling(false);
        can.set_clock_divider(ClockDivider::_2);
        can.set_frame_transmit(hal::fdcan::config::FrameTransmissionConfig::AllowFdCan);

        let btr = NominalBitTiming {
            prescaler: NonZeroU16::new(5).unwrap(),
            seg1: NonZeroU8::new(14).unwrap(),
            seg2: NonZeroU8::new(2).unwrap(),
            sync_jump_width: NonZeroU8::new(1).unwrap(),
        };

        can.set_nominal_bit_timing(btr);

        can.set_standard_filter(
            StandardFilterSlot::_0,
            StandardFilter::accept_all_into_fifo0(),
        );
        // can.into_external_loopback()
        can.into_normal()
    };

    // init clock
    let clock = StmClock::new(cp.DWT, cp.DCB, &rcc.clocks);

    let mut session_manager = HeapSessionManager::<CanMetadata, Milliseconds, StmClock>::new();
    session_manager
        .subscribe(Subscription::new(
            TransferKind::Message,
            7509, // TODO check
            7,
            embedded_time::duration::Milliseconds(500),
        ))
        .unwrap();

    let mut node = Node::<_, Can, StmClock>::new(Some(42), session_manager);

    let mut transfer_id = 0u8;
    let mut last_published = clock.try_now().unwrap();

    loop {
        if clock.try_now().unwrap() - last_published
            > embedded_time::duration::Generic::new(1000, StmClock::SCALING_FACTOR)
        {
            // Publish string
            let hello = "Hello Python!";
            let mut str = heapless::Vec::<u8, 13>::new();
            str.extend_from_slice(hello.as_bytes()).unwrap();

            let transfer = Transfer {
                timestamp: clock.try_now().unwrap(),
                priority: Priority::Nominal,
                transfer_kind: TransferKind::Message,
                port_id: 100,
                remote_node_id: None,
                transfer_id,
                payload: &str,
            };

            // unchecked_add is unstable :(
            // unsafe { transfer_id.unchecked_add(1); }
            transfer_id += 1;

            for mut frame in node.transmit(&transfer).unwrap() {}

            last_published = clock.try_now().unwrap();

            led.toggle().unwrap();
            delay_syst.delay(1000.ms());
            led.toggle().unwrap();
        }
    }
}

fn config_rcc(rcc: Rcc) -> Rcc {
    rcc.freeze(
        Config::new(SysClockSrc::PLL)
            .pll_cfg(PllConfig {
                mux: PLLSrc::HSI,
                m: 4,
                n: 85,
                r: 2,
                q: Some(2),
                p: Some(2),
            })
            .ahb_psc(hal::rcc::Prescaler::NotDivided)
            .apb_psc(hal::rcc::Prescaler::NotDivided),
    )
}

#[alloc_error_handler]
fn oom(_: Layout) -> ! {
    loop {}
}