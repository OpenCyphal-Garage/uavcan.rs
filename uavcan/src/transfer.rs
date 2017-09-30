//! This module contains everything related to the transfer protocol that will be used to transmit the uavcan frame
//!
//! The only transfer protocol that is currently supported by the uavcan protocol is CAN2.0B.

use lib::core::convert::{From};


#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransmitError {
    BufferFull,
}

/// `TransferInterface` is an interface to a hardware unit which can communicate over a CAN like transfer protocol
///
/// It's associated with a `TransferFrame` and must be able to receive and transmit this type of frames.
/// The interface must also do ordering of incoming frames after priority defined by the transfer frame ID to avoid priority inversion,
/// while making sure that transfer frames with the same ID is transmitted in the same order as they was added in the transmit buffer.
///
/// Receiving frames must be returned in the same order they were received by the interface.
pub trait TransferInterface {
    /// The TransferFrame associated with this interface.
    type Frame: TransferFrame;

    /// Put a `TransferFrame` in the transfer buffer (or transmit it on the bus) or report an error.
    ///
    /// To avoid priority inversion the new frame needs to be prioritized inside the interface as it would on the bus.
    /// When reprioritizing the `TransferInterface` must for equal ID frames respect the order they were attempted transmitted in.
    /// 
    fn transmit(&self, frame: &Self::Frame) -> Result<(), TransmitError>;

    /// Receive the oldest transfer frame optionally matching an identifier.
    ///
    /// if no identifier is specified, return the oldest frame matching any identifier.
    fn receive(&self, identifier: Option<&FullTransferID>) -> Option<Self::Frame>;

    /// Returns a slice with transfer IDs to all transfer frames where `frame.is_end_frame()` is asserted
    ///
    /// This means that the ID should not be removed from the list until the last frame of a transfer is received
    fn received_completely(&self) -> &[FullTransferID];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FullTransferID {
    pub frame_id: TransferFrameID,
    pub transfer_id: TransferID,
}

/// `TransferFrame` is a CAN like frame that can be sent over a network
///
/// For a frame to work it need to have a 28 bit ID, and a payload of
/// at least 4 bytes. Guarantee that frames are delivered in order
/// and correctness check is needed as well.
///
/// The uavcan protocol defines how this works with a CAN2.0B frame
pub trait TransferFrame {
    /// Maximum data length the transfer protocol supports.
    const MAX_DATA_LENGTH: usize;

    /// Create a new TransferFrame with id: id, and length 0.
    /// Data length can be changed with `set_data_length(&self)`.
    /// Data can be changed with `data_as_mut(&mut self)`.
    fn new(id: TransferFrameID) -> Self;

    /// Returns the 28 bit ID of this TransferFrame.
    ///
    /// When deciding which frame that will be transmitted,
    /// the ID is used to prioritze (lower ID means higher priority)
    fn id(&self) -> TransferFrameID;

    /// Returns a slice with the data in this TransferFrame
    ///
    /// Length can be found by checking the length
    /// of this slice `self.data().len()`
    fn data(&self) -> &[u8];

    /// Returns a mutable slice with the data in this TransferFrame
    /// use this method to set/change the data inside this TransferFrame
    fn data_as_mut(&mut self) -> &mut[u8];

    /// Set the data length of this TransferFrame
    ///
    /// ## Panics
    /// `set_data_lengt(&mut self, length: usize)` should panic if `length > T::MAX_DATA_LENGTH`
    fn set_data_length(&mut self, length: usize);
    
    /// Returns the tail byte of the TransferFrame assuming the current length
    fn tail_byte(&self) -> TailByte {
        TailByte::from(*self.data().last().unwrap())
    }

    /// Checks the tail byte if this frame is a start frame and return the result
    fn is_start_frame(&self) -> bool {
        self.tail_byte().start_of_transfer()
    }
    
    /// Checks the tail byte if this frame is an end frame and return the result
    fn is_end_frame(&self) -> bool {
        self.tail_byte().end_of_transfer()
    }
    
    /// Checks the tail byte if this is both a start frame and an end frame and return the result
    fn is_single_frame(&self) -> bool {
        self.is_end_frame() && self.is_start_frame()
    }

    /// Returns the full ID of the frame (both Frame ID and transfer ID)
    fn full_id(&self) -> FullTransferID {
        FullTransferID{frame_id: self.id(), transfer_id: self.tail_byte().transfer_id()} 
    }

}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransferFrameID(u32);

impl From<TransferFrameID> for u32 {
    fn from(id: TransferFrameID) -> u32 {
        let TransferFrameID(value) = id;
        value
    }
}

impl From<u32> for TransferFrameID {
    fn from(value: u32) -> TransferFrameID {
        assert_eq!(value & !0x1fff_ffff, 0);
        TransferFrameID(value)
    }
}


pub struct TailByte(u8);

impl TailByte {
    pub fn new(start_of_transfer: bool, end_of_transfer: bool, toggle: bool, transfer_id: TransferID) -> Self {
        TailByte( ((start_of_transfer as u8)<<7) | ((end_of_transfer as u8)<<6) | ((toggle as u8)<<5) | (u8::from(transfer_id)) )
    }
    
    pub fn start_of_transfer(&self) -> bool {
        let TailByte(value) = *self;
        value & (1<<7) != 0
    }

    pub fn end_of_transfer(&self) -> bool {
        let TailByte(value) = *self;
        value & (1<<6) != 0
    }
    
    pub fn toggle(&self) -> bool {
        let TailByte(value) = *self;
        value & (1<<5) != 0
    }
    
    pub fn transfer_id(&self) -> TransferID {
        let TailByte(value) = *self;
        TransferID(value)
    }
}


impl From<TailByte> for u8 {
    fn from(tb: TailByte) -> u8 {
        let TailByte(value) = tb;
        value
    }
}

impl From<u8> for TailByte {
    fn from(value: u8) -> TailByte {
        TailByte(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransferID(u8);

impl From<TransferID> for u8 {
    fn from(tid: TransferID) -> u8 {
        let TransferID(value) = tid;
        value
    }
}

impl From<u8> for TransferID {
    fn from(value: u8) -> TransferID {
        assert_eq!(value & !0x1f, 0);
        TransferID(value)
    }
}
