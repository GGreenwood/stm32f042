#![feature(used)]
#![feature(const_fn)]
#![no_std]

extern crate numtoa;
extern crate cortex_m;
extern crate cortex_m_rt;

#[macro_use(interrupt)]
extern crate stm32f042;
extern crate volatile_register;

use stm32f042::*;
use core::fmt::Write;
use stm32f042::Interrupt;
use numtoa::NumToA;

use stm32f042::peripherals::i2c::write_data as write_data;


const SSD1306_BYTE_CMD: u8 = 0x00;
const SSD1306_BYTE_DATA: u8 = 0x40;
const SSD1306_BYTE_CMD_SINGLE: u8 = 0x80;

const SSD1306_DISPLAY_RAM: u8 = 0xA4;
const SSD1306_DISPLAY_NORMAL: u8 = 0xA6;
const SSD1306_DISPLAY_OFF: u8 = 0xAE;
const SSD1306_DISPLAY_ON: u8 = 0xAF;

const SSD1306_MEMORY_ADDR_MODE: u8 = 0x20;
const SSD1306_COLUMN_RANGE: u8 = 0x21;
const SSD1306_PAGE_RANGE: u8 = 0x22;

const SSD1306_DISPLAY_START_LINE: u8 = 0x40;
const SSD1306_SCAN_MODE_NORMAL: u8 = 0xC0;
const SSD1306_DISPLAY_OFFSET: u8 = 0xD3;
const SSD1306_PIN_MAP: u8 = 0xDA;

const SSD1306_DISPLAY_CLK_DIV: u8 = 0xD5;
const SSD1306_CHARGE_PUMP: u8 = 0x8D;


fn main() {
    cortex_m::interrupt::free(|cs| {
        let rcc = RCC.borrow(cs);
        let gpioa = GPIOA.borrow(cs);
        let gpiof = GPIOF.borrow(cs);
        let usart1 = stm32f042::USART1.borrow(cs);
        let nvic = NVIC.borrow(cs);
        let i2c = I2C1.borrow(cs);

        /* Enable clock for SYSCFG and USART */
        rcc.apb2enr.modify(|_, w| {
            w.syscfgen().set_bit().usart1en().set_bit()
        });

        /* Enable clock for GPIO Port A, B and F */
        rcc.ahbenr.modify(|_, w| {
            w.iopaen().set_bit().iopben().set_bit().iopfen().set_bit()
        });

        /* Enable clock for I2C1 */
        rcc.apb1enr.modify(|_, w| w.i2c1en().set_bit());

        /* Reset I2C1 */
        rcc.apb1rstr.modify(|_, w| w.i2c1rst().set_bit());
        rcc.apb1rstr.modify(|_, w| w.i2c1rst().clear_bit());

        /* (Re-)configure PB1, PB2 and PB3 as output */
        gpioa.moder.modify(|_, w| unsafe {
            w.moder1().bits(1).moder2().bits(1).moder3().bits(1)
        });

        /* Set alternate function on PF0 and PF1 */
        gpiof.moder.modify(|_, w| unsafe {
            w.moder0().bits(2).moder1().bits(2)
        });

        /* Set AF1 for pin PF0/PF1 to enable I2C */
        gpiof.afrl.modify(|_, w| unsafe {
            w.afrl0().bits(1).afrl1().bits(1)
        });

        /* Set internal pull-up for pin PF0/PF1 */
        gpiof.pupdr.modify(|_, w| unsafe {
            w.pupdr0().bits(1).pupdr1().bits(1)
        });

        /* Set mode to open drain for pin PF0/PF1 */
        gpiof.otyper.modify(
            |_, w| w.ot0().set_bit().ot1().set_bit(),
        );

        /* Set PF0, PF1 to high speed */
        gpiof.ospeedr.modify(|_, w| unsafe {
            w.ospeedr0().bits(3).ospeedr1().bits(3)
        });

        /* Make sure the I2C unit is disabled so we can configure it */
        i2c.cr1.modify(|_, w| w.pe().clear_bit());

        /* Enable I2C signal generator, and configure I2C for 400KHz full speed */
        i2c.timingr.write(|w| unsafe { w.bits(0x0010_0209) });

        /* Enable the I2C processing */
        i2c.cr1.modify(|_, w| w.pe().set_bit());

        /* Set alternate function 1 to to enable USART RX/TX */
        gpioa.moder.modify(|_, w| unsafe {
            w.moder9().bits(2).moder10().bits(2)
        });

        /* Set AF1 for pin 9/10 to enable USART RX/TX */
        gpioa.afrh.modify(|_, w| unsafe {
            w.afrh9().bits(1).afrh10().bits(1)
        });

        /* Set baudrate to 115200 @8MHz */
        usart1.brr.write(|w| unsafe { w.bits(0x045) });

        /* Reset other registers to disable advanced USART features */
        usart1.cr2.reset();
        usart1.cr3.reset();

        /* Enable transmission and receiving as well as the RX IRQ */
        usart1.cr1.modify(|_, w| unsafe { w.bits(0x2D) });

        /* Enable USART IRQ, set prio 0 and clear any pending IRQs */
        nvic.enable(Interrupt::USART1);
        unsafe { nvic.set_priority(Interrupt::USART1, 1) };
        nvic.clear_pending(Interrupt::USART1);

        /* Give display time to settle */
        for _ in 0..500_000 {
            cortex_m::asm::nop()
        }

        /* Initialise SSD1306 display */
        ssd1306_init(i2c);

        /* Print a message on the display */
        ssd1306_pos(i2c, 0, 0);
        ssd1306_print_bytes(i2c, &"Send key over serial for action".as_bytes());

        /* Output a nice message */
        Write::write_str(
            &mut Buffer { cs },
            "\r\nWelcome to the SSD1306 example. Enter any character to update display.\r\n",
        ).unwrap();
    });
}


/* Define an interrupt handler, i.e. function to call when interrupt occurs. */
interrupt!(USART1, usart_receive, locals: {
    count: u32 = 0;
});


struct Buffer<'a> {
    cs: &'a cortex_m::interrupt::CriticalSection,
}


impl<'a> core::fmt::Write for Buffer<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let usart1 = stm32f042::USART1.borrow(self.cs);
        for c in s.as_bytes() {
            /* Wait until the USART is clear to send */
            while usart1.isr.read().txe().bit_is_clear() {}

            /* Write the current character to the output register */
            usart1.tdr.modify(|_, w| unsafe { w.bits(*c as u32) });
        }
        Ok(())
    }
}


fn ssd1306_print_bytes(i2c: &stm32f042::I2C1, bytes: &[u8]) {
    /* A 7x7 font shamelessly borrowed from https://github.com/techninja/MarioChron/ */
    const FONT_7X7: [u8; 672] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,// (space)
        0x00, 0x00, 0x5F, 0x00, 0x00, 0x00, 0x00,// !
        0x00, 0x07, 0x00, 0x07, 0x00, 0x00, 0x00,// "
        0x14, 0x7F, 0x14, 0x7F, 0x14, 0x00, 0x00,// #
        0x24, 0x2A, 0x7F, 0x2A, 0x12, 0x00, 0x00,// $
        0x23, 0x13, 0x08, 0x64, 0x62, 0x00, 0x00,// %
        0x36, 0x49, 0x55, 0x22, 0x50, 0x00, 0x00,// &
        0x00, 0x05, 0x03, 0x00, 0x00, 0x00, 0x00,// '
        0x00, 0x1C, 0x22, 0x41, 0x00, 0x00, 0x00,// (
        0x00, 0x41, 0x22, 0x1C, 0x00, 0x00, 0x00,// )
        0x08, 0x2A, 0x1C, 0x2A, 0x08, 0x00, 0x00,// *
        0x08, 0x08, 0x3E, 0x08, 0x08, 0x00, 0x00,// +
        0x00, 0x50, 0x30, 0x00, 0x00, 0x00, 0x00,// ,
        0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00,// -
        0x00, 0x60, 0x60, 0x00, 0x00, 0x00, 0x00,// .
        0x20, 0x10, 0x08, 0x04, 0x02, 0x00, 0x00,// /
        0x1C, 0x3E, 0x61, 0x41, 0x43, 0x3E, 0x1C,// 0
        0x40, 0x42, 0x7F, 0x7F, 0x40, 0x40, 0x00,// 1
        0x62, 0x73, 0x79, 0x59, 0x5D, 0x4F, 0x46,// 2
        0x20, 0x61, 0x49, 0x4D, 0x4F, 0x7B, 0x31,// 3
        0x18, 0x1C, 0x16, 0x13, 0x7F, 0x7F, 0x10,// 4
        0x27, 0x67, 0x45, 0x45, 0x45, 0x7D, 0x38,// 5
        0x3C, 0x7E, 0x4B, 0x49, 0x49, 0x79, 0x30,// 6
        0x03, 0x03, 0x71, 0x79, 0x0D, 0x07, 0x03,// 7
        0x36, 0x7F, 0x49, 0x49, 0x49, 0x7F, 0x36,// 8
        0x06, 0x4F, 0x49, 0x49, 0x69, 0x3F, 0x1E,// 9
        0x00, 0x36, 0x36, 0x00, 0x00, 0x00, 0x00,// :
        0x00, 0x56, 0x36, 0x00, 0x00, 0x00, 0x00,// ;
        0x00, 0x08, 0x14, 0x22, 0x41, 0x00, 0x00,// <
        0x14, 0x14, 0x14, 0x14, 0x14, 0x00, 0x00,// =
        0x41, 0x22, 0x14, 0x08, 0x00, 0x00, 0x00,// >
        0x02, 0x01, 0x51, 0x09, 0x06, 0x00, 0x00,// ?
        0x32, 0x49, 0x79, 0x41, 0x3E, 0x00, 0x00,// @
        0x7E, 0x11, 0x11, 0x11, 0x7E, 0x00, 0x00,// A
        0x7F, 0x49, 0x49, 0x49, 0x36, 0x00, 0x00,// B
        0x3E, 0x41, 0x41, 0x41, 0x22, 0x00, 0x00,// C
        0x7F, 0x7F, 0x41, 0x41, 0x63, 0x3E, 0x1C,// D
        0x7F, 0x49, 0x49, 0x49, 0x41, 0x00, 0x00,// E
        0x7F, 0x09, 0x09, 0x01, 0x01, 0x00, 0x00,// F
        0x3E, 0x41, 0x41, 0x51, 0x32, 0x00, 0x00,// G
        0x7F, 0x08, 0x08, 0x08, 0x7F, 0x00, 0x00,// H
        0x00, 0x41, 0x7F, 0x41, 0x00, 0x00, 0x00,// I
        0x20, 0x40, 0x41, 0x3F, 0x01, 0x00, 0x00,// J
        0x7F, 0x08, 0x14, 0x22, 0x41, 0x00, 0x00,// K
        0x7F, 0x7F, 0x40, 0x40, 0x40, 0x40, 0x00,// L
        0x7F, 0x02, 0x04, 0x02, 0x7F, 0x00, 0x00,// M
        0x7F, 0x04, 0x08, 0x10, 0x7F, 0x00, 0x00,// N
        0x3E, 0x7F, 0x41, 0x41, 0x41, 0x7F, 0x3E,// O
        0x7F, 0x09, 0x09, 0x09, 0x06, 0x00, 0x00,// P
        0x3E, 0x41, 0x51, 0x21, 0x5E, 0x00, 0x00,// Q
        0x7F, 0x7F, 0x11, 0x31, 0x79, 0x6F, 0x4E,// R
        0x46, 0x49, 0x49, 0x49, 0x31, 0x00, 0x00,// S
        0x01, 0x01, 0x7F, 0x01, 0x01, 0x00, 0x00,// T
        0x3F, 0x40, 0x40, 0x40, 0x3F, 0x00, 0x00,// U
        0x1F, 0x20, 0x40, 0x20, 0x1F, 0x00, 0x00,// V
        0x7F, 0x7F, 0x38, 0x1C, 0x38, 0x7F, 0x7F,// W
        0x63, 0x14, 0x08, 0x14, 0x63, 0x00, 0x00,// X
        0x03, 0x04, 0x78, 0x04, 0x03, 0x00, 0x00,// Y
        0x61, 0x51, 0x49, 0x45, 0x43, 0x00, 0x00,// Z
        0x00, 0x00, 0x7F, 0x41, 0x41, 0x00, 0x00,// [
        0x02, 0x04, 0x08, 0x10, 0x20, 0x00, 0x00,// "\"
        0x41, 0x41, 0x7F, 0x00, 0x00, 0x00, 0x00,// ]
        0x04, 0x02, 0x01, 0x02, 0x04, 0x00, 0x00,// ^
        0x40, 0x40, 0x40, 0x40, 0x40, 0x00, 0x00,// _
        0x00, 0x01, 0x02, 0x04, 0x00, 0x00, 0x00,// `
        0x20, 0x54, 0x54, 0x54, 0x78, 0x00, 0x00,// a
        0x7F, 0x48, 0x44, 0x44, 0x38, 0x00, 0x00,// b
        0x38, 0x44, 0x44, 0x44, 0x20, 0x00, 0x00,// c
        0x38, 0x44, 0x44, 0x48, 0x7F, 0x00, 0x00,// d
        0x38, 0x54, 0x54, 0x54, 0x18, 0x00, 0x00,// e
        0x08, 0x7E, 0x09, 0x01, 0x02, 0x00, 0x00,// f
        0x08, 0x14, 0x54, 0x54, 0x3C, 0x00, 0x00,// g
        0x7F, 0x08, 0x04, 0x04, 0x78, 0x00, 0x00,// h
        0x00, 0x44, 0x7D, 0x40, 0x00, 0x00, 0x00,// i
        0x20, 0x40, 0x44, 0x3D, 0x00, 0x00, 0x00,// j
        0x00, 0x7F, 0x10, 0x28, 0x44, 0x00, 0x00,// k
        0x00, 0x41, 0x7F, 0x40, 0x00, 0x00, 0x00,// l
        0x7C, 0x04, 0x18, 0x04, 0x78, 0x00, 0x00,// m
        0x7C, 0x08, 0x04, 0x04, 0x78, 0x00, 0x00,// n
        0x38, 0x44, 0x44, 0x44, 0x38, 0x00, 0x00,// o
        0x7C, 0x14, 0x14, 0x14, 0x08, 0x00, 0x00,// p
        0x08, 0x14, 0x14, 0x18, 0x7C, 0x00, 0x00,// q
        0x7C, 0x08, 0x04, 0x04, 0x08, 0x00, 0x00,// r
        0x48, 0x54, 0x54, 0x54, 0x20, 0x00, 0x00,// s
        0x04, 0x3F, 0x44, 0x40, 0x20, 0x00, 0x00,// t
        0x3C, 0x40, 0x40, 0x20, 0x7C, 0x00, 0x00,// u
        0x1C, 0x20, 0x40, 0x20, 0x1C, 0x00, 0x00,// v
        0x3C, 0x40, 0x30, 0x40, 0x3C, 0x00, 0x00,// w
        0x00, 0x44, 0x28, 0x10, 0x28, 0x44, 0x00,// x
        0x0C, 0x50, 0x50, 0x50, 0x3C, 0x00, 0x00,// y
        0x44, 0x64, 0x54, 0x4C, 0x44, 0x00, 0x00,// z
        0x00, 0x08, 0x36, 0x41, 0x00, 0x00, 0x00,// {
        0x00, 0x00, 0x7F, 0x00, 0x00, 0x00, 0x00,// |
        0x00, 0x41, 0x36, 0x08, 0x00, 0x00, 0x00,// }
        0x08, 0x08, 0x2A, 0x1C, 0x08, 0x00, 0x00,// ->
        0x08, 0x1C, 0x2A, 0x08, 0x08, 0x00, 0x00 // <-
    ];

    for c in bytes {
        /* Create an array with our I2C instruction and a blank column at the end */
        let mut data: [u8; 9] = [SSD1306_BYTE_DATA, 0, 0, 0, 0, 0, 0, 0, 0];

        /* Calculate our index into the character table above */
        let index = (*c as usize - 0x20) * 7;

        /* Populate the middle of the array with the data from the character array at the right
         * index */
        data[1..8].copy_from_slice(&FONT_7X7[index..index + 7]);

        /* Write it out to the I2C bus */
        write_data(i2c, 0x3C, &data);
    }
}


fn read_char(usart1: &stm32f042::USART1) -> u8 {
    /* Read the received value from the USART register */
    let c = {
        /* Check for overflow */
        if usart1.isr.read().ore().bit_is_set() {
            usart1.icr.modify(|_, w| w.orecf().set_bit());
            usart1.rdr.read().bits()
        }
        /* Check if the USART received something */
        else if usart1.isr.read().rxne().bit_is_set() {
            usart1.rdr.read().bits()
        }
        /* Otherwise we'll set a dummy value */
        else {
            0
        }
    };

    /* If value is not the dummy value: echo it back to the serial line */
    if c != 0 {
        /* Wait until the USART is clear to send */
        while usart1.isr.read().txe().bit_is_clear() {}

        /* Write the current character to the output register */
        usart1.tdr.modify(|_, w| unsafe { w.bits(c) });
    }

    c as u8
}


/* Initialise display with some useful values */
fn ssd1306_init(i2c: &I2C1) {
    write_data(i2c, 0x3C, &[SSD1306_BYTE_CMD_SINGLE, SSD1306_DISPLAY_OFF]);
    write_data(
        i2c,
        0x3C,
        &[SSD1306_BYTE_CMD_SINGLE, SSD1306_DISPLAY_CLK_DIV, 0x80],
    );
    write_data(
        i2c,
        0x3C,
        &[SSD1306_BYTE_CMD_SINGLE, SSD1306_SCAN_MODE_NORMAL],
    );
    write_data(
        i2c,
        0x3C,
        &[SSD1306_BYTE_CMD_SINGLE, SSD1306_DISPLAY_OFFSET, 0x00, 0x00],
    );
    write_data(
        i2c,
        0x3C,
        &[SSD1306_BYTE_CMD_SINGLE, SSD1306_MEMORY_ADDR_MODE, 0x00],
    );
    write_data(
        i2c,
        0x3C,
        &[SSD1306_BYTE_CMD_SINGLE, SSD1306_DISPLAY_START_LINE, 0x00],
    );
    write_data(
        i2c,
        0x3C,
        &[SSD1306_BYTE_CMD_SINGLE, SSD1306_CHARGE_PUMP, 0x14],
    );
    write_data(i2c, 0x3C, &[SSD1306_BYTE_CMD_SINGLE, SSD1306_PIN_MAP, 0x12]);
    write_data(i2c, 0x3C, &[SSD1306_BYTE_CMD_SINGLE, SSD1306_DISPLAY_RAM]);
    write_data(
        i2c,
        0x3C,
        &[SSD1306_BYTE_CMD_SINGLE, SSD1306_DISPLAY_NORMAL],
    );
    write_data(i2c, 0x3C, &[SSD1306_BYTE_CMD_SINGLE, SSD1306_DISPLAY_ON]);

    let data = [
        SSD1306_BYTE_DATA,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
    ];

    for _ in 0..128 {
        write_data(i2c, 0x3C, &data);
    }
}


/* Position cursor at specified x, y block coordinate (multiple of 8) */
fn ssd1306_pos(i2c: &I2C1, x: u8, y: u8) {
    let data = [
        SSD1306_BYTE_CMD,
        SSD1306_COLUMN_RANGE,
        x * 8,
        0x7F,
        SSD1306_PAGE_RANGE,
        y,
        0x07,
    ];
    write_data(i2c, 0x3C, &data);
}


/* The IRQ handler triggered by a received character in USART buffer, this will send out something
 * to the I2C display */
fn usart_receive(l: &mut USART1::Locals) {
    cortex_m::interrupt::free(|cs| {
        let usart1 = stm32f042::USART1.borrow(cs);
        let i2c = I2C1.borrow(cs);

        l.count += 1;

        /* Read the character that triggered the interrupt from the USART */
        read_char(usart1);

        /* Convert counter into a string */
        let mut buffer = [0u8; 10];
        let count_start = l.count.numtoa(10, &mut buffer);

        /* Position cursor in third row */
        ssd1306_pos(i2c, 0, 3);

        /* Render count on display */
        ssd1306_print_bytes(i2c, &buffer[count_start..]);
    });
}