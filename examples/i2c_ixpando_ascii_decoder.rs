#![feature(used)]
#![feature(const_fn)]
#![no_std]

extern crate cortex_m;

#[macro_use(interrupt)]
extern crate stm32f042;

use stm32f042::peripherals::i2c::write_data;
use stm32f042::peripherals::i2c::read_data;
use stm32f042::peripherals::usart;

use stm32f042::*;
use core::fmt::Write;
use stm32f042::Interrupt;


/* By default the MCP23017 on the Ixpando is configured to address 0x20 */
const I2C_ADDRESS: u8 = 0x20;


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

        /* Set type of all LEDs to output */
        write_data(i2c, I2C_ADDRESS, &[0x00, 0x00]);

        /* Output a nice message */
        let _ = Write::write_str(
            &mut usart::USARTBuffer(cs),
            "\r\nWelcome to the Ixpando ASCII decoder. Hit any key to light up LEDs.\r\n",
        );
    });
}


/* Define an interrupt handler, i.e. function to call when interrupt occurs. Here if we receive a
 * character from the USART well call the handler */
interrupt!(USART1, usart_receive);


/* The IRQ handler triggered by a received character in USART buffer, this will do some I2C stuff
 * when we receive anything */
fn usart_receive() {
    cortex_m::interrupt::free(|cs| {
        let usart1 = stm32f042::USART1.borrow(cs);
        let i2c = I2C1.borrow(cs);
        let mut buf = usart::USARTBuffer(cs);

        /* A byte array of size 1 to store state in */
        let mut state = [0; 1];

        /* Read the character that triggered the interrupt from the USART */
        let c = usart::read_char(usart1, false);

        /* Read the current LED state */
        read_data(i2c, I2C_ADDRESS, 0x12, 1, &mut state);
        let _ = write!(buf, "Previous output state 0x{:x}\r\n", state[0]);

        /* Write new LED state with ASCII value of character */
        write_data(i2c, I2C_ADDRESS, &[0x12, c]);

        /* Read the current LED state */
        read_data(i2c, I2C_ADDRESS, 0x12, 1, &mut state);
        let _ = write!(buf, "New output state 0x{:x}\r\n", state[0]);

        let _ = write!(buf, "\r\nHit any key to do that again.\r\n");
    });
}
