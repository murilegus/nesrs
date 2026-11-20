//! The CPU-side system bus: wires RAM, PPU, APU, controllers and the cartridge
//! together and clocks them as the CPU accesses memory.

use crate::apu::Apu;
use crate::controller::Controller;
use crate::cpu::Bus;
use crate::mapper::Mapper;
use crate::ppu::Ppu;

pub(crate) struct SystemBus {
    pub ram: [u8; 0x800],
    pub ppu: Ppu,
    pub apu: Apu,
    pub mapper: Box<dyn Mapper>,
    pub controllers: [Controller; 2],
    /// CPU cycles elapsed since power-on.
    pub cycles: u64,
    /// Last value on the data bus, returned by reads nothing responds to.
    open_bus: u8,
    nmi_line: bool,
    nmi_edge: bool,
    /// Page latched by a write to `$4014`, waiting for the DMA to run.
    pub oam_dma: Option<u8>,
    /// CPU cycles stolen by DMC sample fetches, not yet accounted for.
    pub stall: u8,
}

impl SystemBus {
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        SystemBus {
            ram: [0; 0x800],
            ppu: Ppu::new(),
            apu: Apu::new(),
            mapper,
            controllers: [Controller::default(), Controller::default()],
            cycles: 0,
            open_bus: 0,
            nmi_line: false,
            nmi_edge: false,
            oam_dma: None,
            stall: 0,
        }
    }

    /// Advances everything but the CPU by one CPU cycle.
    pub fn tick(&mut self) {
        self.cycles += 1;
        for _ in 0..3 {
            self.ppu.tick(&mut *self.mapper);
            let line = self.ppu.nmi_line();
            if line && !self.nmi_line {
                self.nmi_edge = true;
            }
            self.nmi_line = line;
        }
        self.apu.tick();
        if let Some(addr) = self.apu.dmc_fetch_request() {
            let byte = self.mapper.cpu_read(addr).unwrap_or(self.open_bus);
            self.apu.dmc_supply(byte);
            // The fetch halts the CPU for up to four cycles; three is typical.
            self.stall += 3;
        }
    }

    /// Copies a page of CPU memory into sprite RAM, halting the CPU for 513 cycles
    /// (514 when it starts on an odd cycle).
    pub fn run_oam_dma(&mut self, page: u8) {
        self.tick();
        if self.cycles & 1 == 1 {
            self.tick();
        }
        for offset in 0..=255u16 {
            let value = self.read((page as u16) << 8 | offset);
            self.write(0x2004, value);
        }
    }
}

impl Bus for SystemBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.tick();
        let value = match addr {
            0x0000..=0x1FFF => self.ram[addr as usize & 0x7FF],
            0x2000..=0x3FFF => self.ppu.read_register(addr, &mut *self.mapper),
            // The status register is internal to the CPU and does not drive the bus.
            0x4015 => return self.apu.read_status() | (self.open_bus & 0x20),
            0x4016 => self.controllers[0].read() | (self.open_bus & 0xE0),
            0x4017 => self.controllers[1].read() | (self.open_bus & 0xE0),
            0x4000..=0x401F => self.open_bus,
            _ => self.mapper.cpu_read(addr).unwrap_or(self.open_bus),
        };
        self.open_bus = value;
        value
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.tick();
        self.open_bus = value;
        match addr {
            0x0000..=0x1FFF => self.ram[addr as usize & 0x7FF] = value,
            0x2000..=0x3FFF => self.ppu.write_register(addr, value, &mut *self.mapper),
            0x4014 => self.oam_dma = Some(value),
            0x4016 => {
                for controller in &mut self.controllers {
                    controller.write(value);
                }
            }
            0x4000..=0x4017 => self.apu.write(addr, value),
            0x4018..=0x401F => {}
            _ => self.mapper.cpu_write(addr, value),
        }
    }

    fn poll_nmi(&mut self) -> bool {
        std::mem::take(&mut self.nmi_edge)
    }

    fn irq(&mut self) -> bool {
        self.apu.irq() || self.mapper.irq()
    }
}
