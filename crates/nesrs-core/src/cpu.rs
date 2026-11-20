//! Ricoh 2A03 CPU core (a MOS 6502 without decimal mode).
//!
//! The core is cycle-stepped through the bus: every call to [`Bus::read`] or
//! [`Bus::write`] is exactly one CPU cycle, including the dummy accesses real
//! hardware performs. The bus implementation advances the rest of the system
//! on each access, which keeps the PPU and APU in step with the CPU for free.

/// Memory bus as seen by the CPU.
pub trait Bus {
    /// Reads a byte. Takes one CPU cycle.
    fn read(&mut self, addr: u16) -> u8;
    /// Writes a byte. Takes one CPU cycle.
    fn write(&mut self, addr: u16, value: u8);
    /// Returns `true` once for every new NMI request (edge triggered).
    fn poll_nmi(&mut self) -> bool {
        false
    }
    /// Current level of the IRQ line (`true` means asserted).
    fn irq(&mut self) -> bool {
        false
    }
}

/// Processor status flag bits.
pub mod flag {
    pub const C: u8 = 0x01;
    pub const Z: u8 = 0x02;
    pub const I: u8 = 0x04;
    pub const D: u8 = 0x08;
    pub const B: u8 = 0x10;
    pub const U: u8 = 0x20;
    pub const V: u8 = 0x40;
    pub const N: u8 = 0x80;
}
use flag::*;

const NMI_VECTOR: u16 = 0xFFFA;
const RESET_VECTOR: u16 = 0xFFFC;
const IRQ_VECTOR: u16 = 0xFFFE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Ind,
    Izx,
    Izy,
    Rel,
}

#[rustfmt::skip]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Adc, And, Asl, Bcc, Bcs, Beq, Bit, Bmi, Bne, Bpl, Brk, Bvc, Bvs, Clc, Cld, Cli, Clv, Cmp, Cpx,
    Cpy, Dec, Dex, Dey, Eor, Inc, Inx, Iny, Jmp, Jsr, Lda, Ldx, Ldy, Lsr, Nop, Ora, Pha, Php, Pla,
    Plp, Rol, Ror, Rti, Rts, Sbc, Sec, Sed, Sei, Sta, Stx, Sty, Tax, Tay, Tsx, Txa, Txs, Tya,
    // Unofficial opcodes.
    Alr, Anc, Arr, Axs, Dcp, Isc, Jam, Las, Lax, Lxa, Rla, Rra, Sax, Sha, Shx, Shy, Slo, Sre, Tas,
    Xaa,
}

use Mode::*;
use Op::*;

/// Decode table: instruction and addressing mode for each of the 256 opcodes.
#[rustfmt::skip]
const OPCODES: [(Op, Mode); 256] = [
    // 0x00
    (Brk, Imp), (Ora, Izx), (Jam, Imp), (Slo, Izx), (Nop, Zp),  (Ora, Zp),  (Asl, Zp),  (Slo, Zp),
    (Php, Imp), (Ora, Imm), (Asl, Acc), (Anc, Imm), (Nop, Abs), (Ora, Abs), (Asl, Abs), (Slo, Abs),
    // 0x10
    (Bpl, Rel), (Ora, Izy), (Jam, Imp), (Slo, Izy), (Nop, Zpx), (Ora, Zpx), (Asl, Zpx), (Slo, Zpx),
    (Clc, Imp), (Ora, Aby), (Nop, Imp), (Slo, Aby), (Nop, Abx), (Ora, Abx), (Asl, Abx), (Slo, Abx),
    // 0x20
    (Jsr, Abs), (And, Izx), (Jam, Imp), (Rla, Izx), (Bit, Zp),  (And, Zp),  (Rol, Zp),  (Rla, Zp),
    (Plp, Imp), (And, Imm), (Rol, Acc), (Anc, Imm), (Bit, Abs), (And, Abs), (Rol, Abs), (Rla, Abs),
    // 0x30
    (Bmi, Rel), (And, Izy), (Jam, Imp), (Rla, Izy), (Nop, Zpx), (And, Zpx), (Rol, Zpx), (Rla, Zpx),
    (Sec, Imp), (And, Aby), (Nop, Imp), (Rla, Aby), (Nop, Abx), (And, Abx), (Rol, Abx), (Rla, Abx),
    // 0x40
    (Rti, Imp), (Eor, Izx), (Jam, Imp), (Sre, Izx), (Nop, Zp),  (Eor, Zp),  (Lsr, Zp),  (Sre, Zp),
    (Pha, Imp), (Eor, Imm), (Lsr, Acc), (Alr, Imm), (Jmp, Abs), (Eor, Abs), (Lsr, Abs), (Sre, Abs),
    // 0x50
    (Bvc, Rel), (Eor, Izy), (Jam, Imp), (Sre, Izy), (Nop, Zpx), (Eor, Zpx), (Lsr, Zpx), (Sre, Zpx),
    (Cli, Imp), (Eor, Aby), (Nop, Imp), (Sre, Aby), (Nop, Abx), (Eor, Abx), (Lsr, Abx), (Sre, Abx),
    // 0x60
    (Rts, Imp), (Adc, Izx), (Jam, Imp), (Rra, Izx), (Nop, Zp),  (Adc, Zp),  (Ror, Zp),  (Rra, Zp),
    (Pla, Imp), (Adc, Imm), (Ror, Acc), (Arr, Imm), (Jmp, Ind), (Adc, Abs), (Ror, Abs), (Rra, Abs),
    // 0x70
    (Bvs, Rel), (Adc, Izy), (Jam, Imp), (Rra, Izy), (Nop, Zpx), (Adc, Zpx), (Ror, Zpx), (Rra, Zpx),
    (Sei, Imp), (Adc, Aby), (Nop, Imp), (Rra, Aby), (Nop, Abx), (Adc, Abx), (Ror, Abx), (Rra, Abx),
    // 0x80
    (Nop, Imm), (Sta, Izx), (Nop, Imm), (Sax, Izx), (Sty, Zp),  (Sta, Zp),  (Stx, Zp),  (Sax, Zp),
    (Dey, Imp), (Nop, Imm), (Txa, Imp), (Xaa, Imm), (Sty, Abs), (Sta, Abs), (Stx, Abs), (Sax, Abs),
    // 0x90
    (Bcc, Rel), (Sta, Izy), (Jam, Imp), (Sha, Izy), (Sty, Zpx), (Sta, Zpx), (Stx, Zpy), (Sax, Zpy),
    (Tya, Imp), (Sta, Aby), (Txs, Imp), (Tas, Aby), (Shy, Abx), (Sta, Abx), (Shx, Aby), (Sha, Aby),
    // 0xA0
    (Ldy, Imm), (Lda, Izx), (Ldx, Imm), (Lax, Izx), (Ldy, Zp),  (Lda, Zp),  (Ldx, Zp),  (Lax, Zp),
    (Tay, Imp), (Lda, Imm), (Tax, Imp), (Lxa, Imm), (Ldy, Abs), (Lda, Abs), (Ldx, Abs), (Lax, Abs),
    // 0xB0
    (Bcs, Rel), (Lda, Izy), (Jam, Imp), (Lax, Izy), (Ldy, Zpx), (Lda, Zpx), (Ldx, Zpy), (Lax, Zpy),
    (Clv, Imp), (Lda, Aby), (Tsx, Imp), (Las, Aby), (Ldy, Abx), (Lda, Abx), (Ldx, Aby), (Lax, Aby),
    // 0xC0
    (Cpy, Imm), (Cmp, Izx), (Nop, Imm), (Dcp, Izx), (Cpy, Zp),  (Cmp, Zp),  (Dec, Zp),  (Dcp, Zp),
    (Iny, Imp), (Cmp, Imm), (Dex, Imp), (Axs, Imm), (Cpy, Abs), (Cmp, Abs), (Dec, Abs), (Dcp, Abs),
    // 0xD0
    (Bne, Rel), (Cmp, Izy), (Jam, Imp), (Dcp, Izy), (Nop, Zpx), (Cmp, Zpx), (Dec, Zpx), (Dcp, Zpx),
    (Cld, Imp), (Cmp, Aby), (Nop, Imp), (Dcp, Aby), (Nop, Abx), (Cmp, Abx), (Dec, Abx), (Dcp, Abx),
    // 0xE0
    (Cpx, Imm), (Sbc, Izx), (Nop, Imm), (Isc, Izx), (Cpx, Zp),  (Sbc, Zp),  (Inc, Zp),  (Isc, Zp),
    (Inx, Imp), (Sbc, Imm), (Nop, Imp), (Sbc, Imm), (Cpx, Abs), (Sbc, Abs), (Inc, Abs), (Isc, Abs),
    // 0xF0
    (Beq, Rel), (Sbc, Izy), (Jam, Imp), (Isc, Izy), (Nop, Zpx), (Sbc, Zpx), (Inc, Zpx), (Isc, Zpx),
    (Sed, Imp), (Sbc, Aby), (Nop, Imp), (Isc, Aby), (Nop, Abx), (Sbc, Abx), (Inc, Abx), (Isc, Abx),
];

/// CPU register state.
#[derive(Clone, Debug)]
pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    /// Stack pointer (the stack lives at `$0100 + sp`).
    pub sp: u8,
    /// Status register. Bit 5 is always set and the B flag is never stored.
    pub p: u8,
    pub pc: u16,
    /// Set when a JAM/KIL opcode halted the processor; only a reset clears it.
    pub jammed: bool,
    nmi_pending: bool,
    irq_pending: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    /// Creates a CPU in its power-on state. Call [`Cpu::reset`] before stepping.
    pub fn new() -> Self {
        Cpu {
            a: 0,
            x: 0,
            y: 0,
            sp: 0,
            p: U,
            pc: 0,
            jammed: false,
            nmi_pending: false,
            irq_pending: false,
        }
    }

    /// Runs the 7-cycle reset sequence and loads the reset vector.
    pub fn reset(&mut self, bus: &mut impl Bus) {
        self.jammed = false;
        self.nmi_pending = false;
        self.irq_pending = false;
        bus.read(self.pc);
        bus.read(self.pc);
        // The three stack "pushes" of the interrupt sequence are turned into reads.
        for _ in 0..3 {
            bus.read(0x0100 | self.sp as u16);
            self.sp = self.sp.wrapping_sub(1);
        }
        self.p |= I;
        self.pc = self.read16(bus, RESET_VECTOR);
    }

    /// Executes one instruction, or services one pending interrupt.
    pub fn step(&mut self, bus: &mut impl Bus) {
        if self.jammed {
            bus.read(self.pc);
            return;
        }
        if self.nmi_pending {
            self.nmi_pending = false;
            self.interrupt(bus, NMI_VECTOR);
        } else if self.irq_pending {
            self.irq_pending = false;
            self.interrupt(bus, IRQ_VECTOR);
        } else {
            let i_before = self.p & I != 0;
            let opcode = self.fetch(bus);
            self.execute(bus, opcode);
            // IRQs are sampled before the final cycle, so CLI/SEI/PLP take effect one
            // instruction late. RTI restores the flag early enough to count, and BRK
            // never lets an IRQ in right behind it.
            let masked = match opcode {
                0x00 => true,
                0x40 => self.p & I != 0,
                _ => i_before,
            };
            self.irq_pending = !masked && bus.irq();
        }
        if bus.poll_nmi() {
            self.nmi_pending = true;
        }
    }

    // --- Bus helpers -------------------------------------------------------

    fn fetch(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn fetch16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch(bus) as u16;
        let hi = self.fetch(bus) as u16;
        hi << 8 | lo
    }

    fn read16(&mut self, bus: &mut impl Bus, addr: u16) -> u16 {
        let lo = bus.read(addr) as u16;
        let hi = bus.read(addr.wrapping_add(1)) as u16;
        hi << 8 | lo
    }

    fn push(&mut self, bus: &mut impl Bus, value: u8) {
        bus.write(0x0100 | self.sp as u16, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pop(&mut self, bus: &mut impl Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x0100 | self.sp as u16)
    }

    fn interrupt(&mut self, bus: &mut impl Bus, vector: u16) {
        bus.read(self.pc);
        bus.read(self.pc);
        self.push(bus, (self.pc >> 8) as u8);
        self.push(bus, self.pc as u8);
        self.push(bus, self.p | U);
        self.p |= I;
        self.pc = self.read16(bus, vector);
    }

    // --- Addressing --------------------------------------------------------

    /// Adds an index to a base address. A page crossing costs an extra cycle; writes
    /// and read-modify-write instructions always pay it (`always_dummy`).
    fn indexed(&mut self, bus: &mut impl Bus, base: u16, index: u8, always_dummy: bool) -> u16 {
        let addr = base.wrapping_add(index as u16);
        if always_dummy || (base ^ addr) & 0xFF00 != 0 {
            bus.read((base & 0xFF00) | (addr & 0x00FF));
        }
        addr
    }

    /// Resolves the effective address of the operand.
    fn operand_addr(&mut self, bus: &mut impl Bus, mode: Mode, always_dummy: bool) -> u16 {
        match mode {
            Imm => {
                let addr = self.pc;
                self.pc = self.pc.wrapping_add(1);
                addr
            }
            Zp => self.fetch(bus) as u16,
            Zpx => {
                let base = self.fetch(bus);
                bus.read(base as u16);
                base.wrapping_add(self.x) as u16
            }
            Zpy => {
                let base = self.fetch(bus);
                bus.read(base as u16);
                base.wrapping_add(self.y) as u16
            }
            Abs => self.fetch16(bus),
            Abx => {
                let base = self.fetch16(bus);
                self.indexed(bus, base, self.x, always_dummy)
            }
            Aby => {
                let base = self.fetch16(bus);
                self.indexed(bus, base, self.y, always_dummy)
            }
            Izx => {
                let zp = self.fetch(bus);
                bus.read(zp as u16);
                let ptr = zp.wrapping_add(self.x);
                let lo = bus.read(ptr as u16) as u16;
                let hi = bus.read(ptr.wrapping_add(1) as u16) as u16;
                hi << 8 | lo
            }
            Izy => {
                let zp = self.fetch(bus);
                let lo = bus.read(zp as u16) as u16;
                let hi = bus.read(zp.wrapping_add(1) as u16) as u16;
                self.indexed(bus, hi << 8 | lo, self.y, always_dummy)
            }
            Imp | Acc | Ind | Rel => unreachable!("mode {mode:?} has no operand address"),
        }
    }

    fn load(&mut self, bus: &mut impl Bus, mode: Mode) -> u8 {
        let addr = self.operand_addr(bus, mode, false);
        bus.read(addr)
    }

    fn store(&mut self, bus: &mut impl Bus, mode: Mode, value: u8) {
        let addr = self.operand_addr(bus, mode, true);
        bus.write(addr, value);
    }

    /// Read-modify-write: reads the operand, writes it back unchanged, then writes
    /// the result. Returns the result.
    fn modify(&mut self, bus: &mut impl Bus, mode: Mode, f: fn(&mut Cpu, u8) -> u8) -> u8 {
        if mode == Acc {
            bus.read(self.pc);
            let result = f(self, self.a);
            self.a = result;
            return result;
        }
        let addr = self.operand_addr(bus, mode, true);
        let value = bus.read(addr);
        bus.write(addr, value);
        let result = f(self, value);
        bus.write(addr, result);
        result
    }

    /// Stores `value & (high byte of the base address + 1)`: the shared behaviour of
    /// the unstable SHA/SHX/SHY/TAS opcodes.
    fn store_high(&mut self, bus: &mut impl Bus, mode: Mode, value: u8) {
        let index = if mode == Abx { self.x } else { self.y };
        let addr = self.operand_addr(bus, mode, true);
        let base_hi = (addr.wrapping_sub(index as u16) >> 8) as u8;
        bus.write(addr, value & base_hi.wrapping_add(1));
    }

    fn branch(&mut self, bus: &mut impl Bus, condition: bool) {
        let offset = self.fetch(bus) as i8;
        if condition {
            bus.read(self.pc);
            let target = self.pc.wrapping_add(offset as u16);
            if (target ^ self.pc) & 0xFF00 != 0 {
                bus.read((self.pc & 0xFF00) | (target & 0x00FF));
            }
            self.pc = target;
        }
    }

    // --- Flag and arithmetic helpers ---------------------------------------

    fn set(&mut self, mask: u8, on: bool) {
        if on {
            self.p |= mask;
        } else {
            self.p &= !mask;
        }
    }

    fn zn(&mut self, value: u8) -> u8 {
        self.set(Z, value == 0);
        self.set(N, value & 0x80 != 0);
        value
    }

    fn adc(&mut self, value: u8) {
        let sum = self.a as u16 + value as u16 + (self.p & C) as u16;
        let result = sum as u8;
        self.set(C, sum > 0xFF);
        self.set(V, !(self.a ^ value) & (self.a ^ result) & 0x80 != 0);
        self.a = self.zn(result);
    }

    fn compare(&mut self, register: u8, value: u8) {
        self.set(C, register >= value);
        self.zn(register.wrapping_sub(value));
    }

    fn asl(&mut self, value: u8) -> u8 {
        self.set(C, value & 0x80 != 0);
        self.zn(value << 1)
    }

    fn lsr(&mut self, value: u8) -> u8 {
        self.set(C, value & 0x01 != 0);
        self.zn(value >> 1)
    }

    fn rol(&mut self, value: u8) -> u8 {
        let carry = self.p & C;
        self.set(C, value & 0x80 != 0);
        self.zn(value << 1 | carry)
    }

    fn ror(&mut self, value: u8) -> u8 {
        let carry = (self.p & C) << 7;
        self.set(C, value & 0x01 != 0);
        self.zn(value >> 1 | carry)
    }

    fn inc(&mut self, value: u8) -> u8 {
        self.zn(value.wrapping_add(1))
    }

    fn dec(&mut self, value: u8) -> u8 {
        self.zn(value.wrapping_sub(1))
    }

    // --- Execution ---------------------------------------------------------

    fn execute(&mut self, bus: &mut impl Bus, opcode: u8) {
        let (op, mode) = OPCODES[opcode as usize];
        match op {
            // Loads and stores.
            Lda => {
                let v = self.load(bus, mode);
                self.a = self.zn(v);
            }
            Ldx => {
                let v = self.load(bus, mode);
                self.x = self.zn(v);
            }
            Ldy => {
                let v = self.load(bus, mode);
                self.y = self.zn(v);
            }
            Sta => self.store(bus, mode, self.a),
            Stx => self.store(bus, mode, self.x),
            Sty => self.store(bus, mode, self.y),

            // Register transfers.
            Tax => self.implied(bus, |c| c.x = c.zn(c.a)),
            Tay => self.implied(bus, |c| c.y = c.zn(c.a)),
            Txa => self.implied(bus, |c| c.a = c.zn(c.x)),
            Tya => self.implied(bus, |c| c.a = c.zn(c.y)),
            Tsx => self.implied(bus, |c| c.x = c.zn(c.sp)),
            Txs => self.implied(bus, |c| c.sp = c.x),

            // Stack.
            Pha => {
                bus.read(self.pc);
                self.push(bus, self.a);
            }
            Php => {
                bus.read(self.pc);
                self.push(bus, self.p | B | U);
            }
            Pla => {
                bus.read(self.pc);
                bus.read(0x0100 | self.sp as u16);
                let v = self.pop(bus);
                self.a = self.zn(v);
            }
            Plp => {
                bus.read(self.pc);
                bus.read(0x0100 | self.sp as u16);
                self.p = (self.pop(bus) & !B) | U;
            }

            // Logic and arithmetic.
            And => {
                let v = self.load(bus, mode);
                self.a = self.zn(self.a & v);
            }
            Eor => {
                let v = self.load(bus, mode);
                self.a = self.zn(self.a ^ v);
            }
            Ora => {
                let v = self.load(bus, mode);
                self.a = self.zn(self.a | v);
            }
            Bit => {
                let v = self.load(bus, mode);
                self.set(Z, self.a & v == 0);
                self.set(N, v & 0x80 != 0);
                self.set(V, v & 0x40 != 0);
            }
            Adc => {
                let v = self.load(bus, mode);
                self.adc(v);
            }
            Sbc => {
                let v = self.load(bus, mode);
                self.adc(!v);
            }
            Cmp => {
                let v = self.load(bus, mode);
                self.compare(self.a, v);
            }
            Cpx => {
                let v = self.load(bus, mode);
                self.compare(self.x, v);
            }
            Cpy => {
                let v = self.load(bus, mode);
                self.compare(self.y, v);
            }

            // Increments, decrements and shifts.
            Inc => {
                self.modify(bus, mode, Cpu::inc);
            }
            Dec => {
                self.modify(bus, mode, Cpu::dec);
            }
            Inx => self.implied(bus, |c| c.x = c.inc(c.x)),
            Iny => self.implied(bus, |c| c.y = c.inc(c.y)),
            Dex => self.implied(bus, |c| c.x = c.dec(c.x)),
            Dey => self.implied(bus, |c| c.y = c.dec(c.y)),
            Asl => {
                self.modify(bus, mode, Cpu::asl);
            }
            Lsr => {
                self.modify(bus, mode, Cpu::lsr);
            }
            Rol => {
                self.modify(bus, mode, Cpu::rol);
            }
            Ror => {
                self.modify(bus, mode, Cpu::ror);
            }

            // Jumps and subroutines.
            Jmp => {
                let target = self.fetch16(bus);
                self.pc = if mode == Ind {
                    // The pointer's high byte is fetched without carrying into the page.
                    let lo = bus.read(target) as u16;
                    let hi = bus.read((target & 0xFF00) | (target.wrapping_add(1) & 0x00FF));
                    (hi as u16) << 8 | lo
                } else {
                    target
                };
            }
            Jsr => {
                let lo = self.fetch(bus) as u16;
                bus.read(0x0100 | self.sp as u16);
                self.push(bus, (self.pc >> 8) as u8);
                self.push(bus, self.pc as u8);
                let hi = bus.read(self.pc) as u16;
                self.pc = hi << 8 | lo;
            }
            Rts => {
                bus.read(self.pc);
                bus.read(0x0100 | self.sp as u16);
                let lo = self.pop(bus) as u16;
                let hi = self.pop(bus) as u16;
                self.pc = hi << 8 | lo;
                bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
            }
            Rti => {
                bus.read(self.pc);
                bus.read(0x0100 | self.sp as u16);
                self.p = (self.pop(bus) & !B) | U;
                let lo = self.pop(bus) as u16;
                let hi = self.pop(bus) as u16;
                self.pc = hi << 8 | lo;
            }
            Brk => {
                self.fetch(bus); // padding byte
                self.push(bus, (self.pc >> 8) as u8);
                self.push(bus, self.pc as u8);
                self.push(bus, self.p | B | U);
                self.p |= I;
                self.pc = self.read16(bus, IRQ_VECTOR);
            }

            // Branches.
            Bcc => self.branch(bus, self.p & C == 0),
            Bcs => self.branch(bus, self.p & C != 0),
            Bne => self.branch(bus, self.p & Z == 0),
            Beq => self.branch(bus, self.p & Z != 0),
            Bpl => self.branch(bus, self.p & N == 0),
            Bmi => self.branch(bus, self.p & N != 0),
            Bvc => self.branch(bus, self.p & V == 0),
            Bvs => self.branch(bus, self.p & V != 0),

            // Flag operations.
            Clc => self.implied(bus, |c| c.p &= !C),
            Cld => self.implied(bus, |c| c.p &= !D),
            Cli => self.implied(bus, |c| c.p &= !I),
            Clv => self.implied(bus, |c| c.p &= !V),
            Sec => self.implied(bus, |c| c.p |= C),
            Sed => self.implied(bus, |c| c.p |= D),
            Sei => self.implied(bus, |c| c.p |= I),

            Nop => {
                if mode == Imp {
                    bus.read(self.pc);
                } else {
                    self.load(bus, mode);
                }
            }

            // Unofficial opcodes.
            Lax => {
                let v = self.load(bus, mode);
                self.a = self.zn(v);
                self.x = v;
            }
            Sax => self.store(bus, mode, self.a & self.x),
            Slo => {
                let v = self.modify(bus, mode, Cpu::asl);
                self.a = self.zn(self.a | v);
            }
            Rla => {
                let v = self.modify(bus, mode, Cpu::rol);
                self.a = self.zn(self.a & v);
            }
            Sre => {
                let v = self.modify(bus, mode, Cpu::lsr);
                self.a = self.zn(self.a ^ v);
            }
            Rra => {
                let v = self.modify(bus, mode, Cpu::ror);
                self.adc(v);
            }
            Dcp => {
                let v = self.modify(bus, mode, |_, v| v.wrapping_sub(1));
                self.compare(self.a, v);
            }
            Isc => {
                let v = self.modify(bus, mode, |_, v| v.wrapping_add(1));
                self.adc(!v);
            }
            Anc => {
                let v = self.load(bus, mode);
                self.a = self.zn(self.a & v);
                self.set(C, self.a & 0x80 != 0);
            }
            Alr => {
                let v = self.load(bus, mode);
                self.a = self.lsr(self.a & v);
            }
            Arr => {
                let v = self.load(bus, mode);
                self.a = self.zn((self.a & v) >> 1 | (self.p & C) << 7);
                self.set(C, self.a & 0x40 != 0);
                self.set(V, (self.a >> 6 ^ self.a >> 5) & 1 != 0);
            }
            Axs => {
                let v = self.load(bus, mode);
                let ax = self.a & self.x;
                self.set(C, ax >= v);
                self.x = self.zn(ax.wrapping_sub(v));
            }
            Xaa => {
                let v = self.load(bus, mode);
                self.a = self.zn((self.a | 0xEE) & self.x & v);
            }
            Lxa => {
                let v = self.load(bus, mode);
                self.a = self.zn((self.a | 0xEE) & v);
                self.x = self.a;
            }
            Las => {
                let v = self.load(bus, mode) & self.sp;
                self.a = self.zn(v);
                self.x = v;
                self.sp = v;
            }
            Sha => self.store_high(bus, mode, self.a & self.x),
            Shx => self.store_high(bus, mode, self.x),
            Shy => self.store_high(bus, mode, self.y),
            Tas => {
                self.sp = self.a & self.x;
                self.store_high(bus, mode, self.sp);
            }
            Jam => {
                bus.read(self.pc);
                self.pc = self.pc.wrapping_sub(1);
                self.jammed = true;
            }
        }
    }

    /// Two-cycle implied instruction: a dummy read of the next byte, then `f`.
    fn implied(&mut self, bus: &mut impl Bus, f: fn(&mut Cpu)) {
        bus.read(self.pc);
        f(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 64 KiB of flat RAM that counts bus accesses (= CPU cycles).
    struct RamBus {
        mem: Vec<u8>,
        cycles: u64,
        irq: bool,
        nmi: bool,
    }

    impl RamBus {
        fn new(program: &[u8]) -> Self {
            let mut mem = vec![0u8; 0x10000];
            mem[0x8000..0x8000 + program.len()].copy_from_slice(program);
            mem[0xFFFC] = 0x00;
            mem[0xFFFD] = 0x80;
            RamBus { mem, cycles: 0, irq: false, nmi: false }
        }
    }

    impl Bus for RamBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.cycles += 1;
            self.mem[addr as usize]
        }
        fn write(&mut self, addr: u16, value: u8) {
            self.cycles += 1;
            self.mem[addr as usize] = value;
        }
        fn poll_nmi(&mut self) -> bool {
            std::mem::take(&mut self.nmi)
        }
        fn irq(&mut self) -> bool {
            self.irq
        }
    }

    fn boot(program: &[u8]) -> (Cpu, RamBus) {
        let mut bus = RamBus::new(program);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        (cpu, bus)
    }

    #[test]
    fn reset_takes_seven_cycles_and_loads_vector() {
        let (cpu, bus) = boot(&[]);
        assert_eq!(bus.cycles, 7);
        assert_eq!(cpu.pc, 0x8000);
        assert_eq!(cpu.sp, 0xFD);
        assert_eq!(cpu.p, 0x24);
    }

    /// Base cycle count of every opcode (no page crossing, branches not taken).
    #[rustfmt::skip]
    const CYCLES: [u8; 256] = [
        7, 6, 0, 8, 3, 3, 5, 5, 3, 2, 2, 2, 4, 4, 6, 6, 2, 5, 0, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
        6, 6, 0, 8, 3, 3, 5, 5, 4, 2, 2, 2, 4, 4, 6, 6, 2, 5, 0, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
        6, 6, 0, 8, 3, 3, 5, 5, 3, 2, 2, 2, 3, 4, 6, 6, 2, 5, 0, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
        6, 6, 0, 8, 3, 3, 5, 5, 4, 2, 2, 2, 5, 4, 6, 6, 2, 5, 0, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
        2, 6, 2, 6, 3, 3, 3, 3, 2, 2, 2, 2, 4, 4, 4, 4, 2, 6, 0, 6, 4, 4, 4, 4, 2, 5, 2, 5, 5, 5, 5, 5,
        2, 6, 2, 6, 3, 3, 3, 3, 2, 2, 2, 2, 4, 4, 4, 4, 2, 5, 0, 5, 4, 4, 4, 4, 2, 4, 2, 4, 4, 4, 4, 4,
        2, 6, 2, 8, 3, 3, 5, 5, 2, 2, 2, 2, 4, 4, 6, 6, 2, 5, 0, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
        2, 6, 2, 8, 3, 3, 5, 5, 2, 2, 2, 2, 4, 4, 6, 6, 2, 5, 0, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
    ];

    #[test]
    fn every_opcode_takes_its_documented_cycle_count() {
        for opcode in 0..=255u8 {
            let expected = CYCLES[opcode as usize] as u64;
            if expected == 0 {
                continue; // JAM
            }
            // Operands of zero keep indexed modes on one page, and the flags after
            // reset make every conditional branch except BNE/BPL/BCC/BVC fall through.
            let (mut cpu, mut bus) = boot(&[opcode, 0x00, 0x02]);
            let taken = matches!(opcode, 0x10 | 0x50 | 0x90 | 0xD0);
            let before = bus.cycles;
            cpu.step(&mut bus);
            let expected = if taken { expected + 1 } else { expected };
            assert_eq!(bus.cycles - before, expected, "opcode {opcode:#04X}");
        }
    }

    #[test]
    fn page_crossing_costs_a_cycle_only_for_reads() {
        // LDA $80FF,X with X = 1 crosses a page: 5 cycles.
        let (mut cpu, mut bus) = boot(&[0xA2, 0x01, 0xBD, 0xFF, 0x80, 0x9D, 0xFF, 0x80]);
        cpu.step(&mut bus);
        let before = bus.cycles;
        cpu.step(&mut bus);
        assert_eq!(bus.cycles - before, 5);
        // STA $80FF,X always takes 5.
        let before = bus.cycles;
        cpu.step(&mut bus);
        assert_eq!(bus.cycles - before, 5);
    }

    #[test]
    fn adc_and_sbc_set_carry_and_overflow() {
        // LDA #$50; ADC #$50 -> $A0, V set, C clear. SBC #$F0 (with C clear) -> $AF.
        let (mut cpu, mut bus) = boot(&[0xA9, 0x50, 0x69, 0x50, 0xE9, 0xF0]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0xA0);
        assert_eq!(cpu.p & (C | V | N | Z), V | N);
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0xAF);
        assert_eq!(cpu.p & (C | V), 0);
    }

    #[test]
    fn jsr_and_rts_round_trip() {
        // JSR $8005; LDX #$11 (return lands here); at $8005: LDA #$42; RTS
        let (mut cpu, mut bus) = boot(&[0x20, 0x05, 0x80, 0xA2, 0x11, 0xA9, 0x42, 0x60]);
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x8005);
        assert_eq!(cpu.sp, 0xFB);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x8003);
        cpu.step(&mut bus);
        assert_eq!((cpu.a, cpu.x, cpu.sp), (0x42, 0x11, 0xFD));
    }

    #[test]
    fn jmp_indirect_wraps_within_the_page() {
        let (mut cpu, mut bus) = boot(&[0x6C, 0xFF, 0x02]);
        bus.mem[0x02FF] = 0x34;
        bus.mem[0x0200] = 0x12; // high byte comes from $0200, not $0300
        bus.mem[0x0300] = 0xFF;
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x1234);
    }

    #[test]
    fn branch_cycles_depend_on_target_page() {
        // BNE +2 (taken, same page): 3 cycles.
        let (mut cpu, mut bus) = boot(&[0xD0, 0x02]);
        let before = bus.cycles;
        cpu.step(&mut bus);
        assert_eq!(bus.cycles - before, 3);
        assert_eq!(cpu.pc, 0x8004);
        // BNE -128 (taken, previous page): 4 cycles.
        let (mut cpu, mut bus) = boot(&[0xD0, 0x80]);
        let before = bus.cycles;
        cpu.step(&mut bus);
        assert_eq!(bus.cycles - before, 4);
        assert_eq!(cpu.pc, 0x7F82);
    }

    #[test]
    fn irq_is_delayed_one_instruction_after_cli() {
        // CLI; NOP; NOP with the IRQ line held low from the start.
        let (mut cpu, mut bus) = boot(&[0x58, 0xEA, 0xEA]);
        bus.mem[0xFFFE] = 0x00;
        bus.mem[0xFFFF] = 0x90;
        bus.irq = true;
        cpu.step(&mut bus); // CLI: the old I flag still masks the IRQ
        cpu.step(&mut bus); // NOP runs before the IRQ is taken
        assert_eq!(cpu.pc, 0x8002);
        let before = bus.cycles;
        cpu.step(&mut bus); // IRQ sequence
        assert_eq!(bus.cycles - before, 7);
        assert_eq!(cpu.pc, 0x9000);
        assert_ne!(cpu.p & I, 0);
        // The pushed status has B clear.
        assert_eq!(bus.mem[0x01FB] & B, 0);
    }

    #[test]
    fn nmi_is_serviced_after_the_current_instruction() {
        let (mut cpu, mut bus) = boot(&[0xEA, 0xEA]);
        bus.mem[0xFFFA] = 0x00;
        bus.mem[0xFFFB] = 0xA0;
        bus.nmi = true;
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x8001);
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0xA000);
    }

    #[test]
    fn brk_pushes_status_with_b_set_and_rti_returns() {
        let (mut cpu, mut bus) = boot(&[0x00, 0xFF, 0xEA]);
        bus.mem[0xFFFE] = 0x00;
        bus.mem[0xFFFF] = 0x90;
        bus.mem[0x9000] = 0x40; // RTI
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x9000);
        assert_ne!(bus.mem[0x01FB] & B, 0);
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x8002); // BRK skips its padding byte
        assert_eq!(cpu.sp, 0xFD);
    }

    #[test]
    fn unofficial_lax_sax_dcp_isc() {
        // LAX #imm is unstable, so use LAX zp. $10 holds $5A.
        let (mut cpu, mut bus) = boot(&[0xA7, 0x10, 0x87, 0x11, 0xC7, 0x10, 0xE7, 0x10]);
        bus.mem[0x10] = 0x5A;
        cpu.step(&mut bus);
        assert_eq!((cpu.a, cpu.x), (0x5A, 0x5A));
        cpu.step(&mut bus); // SAX $11
        assert_eq!(bus.mem[0x11], 0x5A);
        cpu.step(&mut bus); // DCP $10 -> $59, compare A($5A) with it
        assert_eq!(bus.mem[0x10], 0x59);
        assert_ne!(cpu.p & C, 0);
        cpu.step(&mut bus); // ISC $10 -> $5A, A = $5A - $5A = 0
        assert_eq!(bus.mem[0x10], 0x5A);
        assert_eq!(cpu.a, 0x00);
        assert_ne!(cpu.p & Z, 0);
    }

    #[test]
    fn jam_halts_until_reset() {
        let (mut cpu, mut bus) = boot(&[0x02, 0xEA]);
        cpu.step(&mut bus);
        assert!(cpu.jammed);
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x8000);
        cpu.reset(&mut bus);
        assert!(!cpu.jammed);
    }
}
