pub trait LedController {

    const BLACK: [u8; 4] = [0, 0, 0, 0];
    const LIGHT_GREEN: [u8; 4] = [0, 60, 0, 0];
    const DARK_GREEN: [u8; 4] = [0, 20, 0, 0];
    const ORANGE: [u8; 4] = [0, 60, 255, 0];
    const RED: [u8; 4] = [0, 0, 255, 0];

    /// Sets all the LEDs to the provided [RawColor].
    ///
    /// # Panics
    ///
    /// Panics if there is an issue with setting the color.
    fn set_all_to(&mut self, color: [u8; 4]) -> ();
}

use rs_ws281x::*;

pub struct RPILedController {
    controller: Controller
}

impl LedController for RPILedController {

    fn set_all_to(&mut self, color: [u8; 4]) -> () {
        let leds = self.controller.leds_mut(0);
        for led in leds {
            *led = color
        }
        self.controller.render().unwrap();
    }
}

impl RPILedController {

    const NUM_LEDS: i32 = 10;
    const LED_PIN: i32 = 18;

    pub fn new() -> Self {
        RPILedController {
            controller: ControllerBuilder::new()
            .freq(800_000)
            .dma(10)
            .channel(
                0, // Channel Index
                ChannelBuilder::new()
                    .pin(Self::LED_PIN)
                    .count(Self::NUM_LEDS)
                    .strip_type(StripType::Ws2812)
                    .brightness(100) // default: 255
                    .build(),
            )
            .build()
            .unwrap()
        }
    }

}