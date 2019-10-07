// Copyright © 2019 Intel Corporation
//
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! [Virtual Function I/O (VFIO) API](https://www.kernel.org/doc/Documentation/vfio.txt)
//!
//! Many modern system now provide DMA and interrupt remapping facilities to help ensure I/O
//! devices behave within the boundaries they've been allotted. This includes x86 hardware with
//! AMD-Vi and Intel VT-d, POWER systems with Partitionable Endpoints (PEs) and embedded PowerPC
//! systems such as Freescale PAMU. The VFIO driver is an IOMMU/device agnostic framework for
//! exposing direct device access to userspace, in a secure, IOMMU protected environment.
//! In other words, the VFIO framework allows safe, non-privileged, userspace drivers.
//!
//! Why do we want that?  Virtual machines often make use of direct device access ("device
//! assignment") when configured for the highest possible I/O performance. From a device and host
//! perspective, this simply turns the VM into a userspace driver, with the benefits of
//! significantly reduced latency, higher bandwidth, and direct use of bare-metal device drivers.
//!
//! Devices are the main target of any I/O driver.  Devices typically create a programming
//! interface made up of I/O access, interrupts, and DMA.  Without going into the details of each
//! of these, DMA is by far the most critical aspect for maintaining a secure environment as
//! allowing a device read-write access to system memory imposes the greatest risk to the overall
//! system integrity.
//!
//! To help mitigate this risk, many modern IOMMUs now incorporate isolation properties into what
//! was, in many cases, an interface only meant for translation (ie. solving the addressing
//! problems of devices with limited address spaces).  With this, devices can now be isolated
//! from each other and from arbitrary memory access, thus allowing things like secure direct
//! assignment of devices into virtual machines.
//!
//! While for the most part an IOMMU may have device level granularity, any system is susceptible
//! to reduced granularity. The IOMMU API therefore supports a notion of IOMMU groups. A group is
//! a set of devices which is isolatable from all other devices in the system. Groups are therefore
//! the unit of ownership used by VFIO.
//!
//! While the group is the minimum granularity that must be used to ensure secure user access, it's
//! not necessarily the preferred granularity. In IOMMUs which make use of page tables, it may be
//! possible to share a set of page tables between different groups, reducing the overhead both to
//! the platform (reduced TLB thrashing, reduced duplicate page tables), and to the user
//! (programming only a single set of translations). For this reason, VFIO makes use of a container
//! class, which may hold one or more groups. A container is created by simply opening the
//! /dev/vfio/vfio character device.
//!
//! This crate is a safe wrapper around the Linux kernel's VFIO interfaces, which offering safe
//! wrappers for:
//! - [VFIO Container](struct.VfioContainer.html) using the `VfioContainer` structure
//! - [VFIO Device](struct.VfioDevice.html) using the `VfioDevice` structure
//!
//! # Platform support
//!
//! - x86_64
//!
//! **NOTE:** The list of available ioctls is not exhaustive.

#![deny(missing_docs)]

#[macro_use]
extern crate vmm_sys_util;
extern crate vm_memory;

use vm_memory::{
    GuestAddress, GuestMemory, GuestMemoryRegion, MemoryRegionAddress,
};


mod fam;
mod vfio_device;
mod vfio_ioctls;
mod dma_mapping;

pub use vfio_device::{VfioContainer, VfioDevice, VfioError, VfioIrq};
pub use dma_mapping::VfioDmaMapping;

/// Trait meant for triggering the DMA mapping update related to an external
/// device not managed fully through virtio. It is dedicated to virtio-iommu
/// in order to trigger the map update anytime the mapping is updated from the
/// guest.
pub trait ExternalDmaMapping: Send + Sync {
    /// Map a memory range
    fn map(&self, iova: u64, gpa: u64, size: u64) -> std::result::Result<(), std::io::Error>;

    /// Unmap a memory range
    fn unmap(&self, iova: u64, size: u64) -> std::result::Result<(), std::io::Error>;
}

fn get_region_host_address_range<M: GuestMemoryRegion>(
    region: &M,
    addr: MemoryRegionAddress,
    size: usize,
) -> Option<*mut u8> {
    region.check_address(addr).and_then(|addr| {
        region
            .checked_offset(addr, size)
            .map(|_| region.get_host_address(addr).unwrap())
    })
}

/// Convert an absolute address into an address space (GuestMemory)
/// to a host pointer and verify that the provided size define a valid
/// range within a single memory region.
/// Return None if it is out of bounds or if addr+size overlaps a single region.
///
/// This is a temporary vm-memory wrapper.
pub fn get_host_address_range<M: GuestMemory>(
    mem: &M,
    addr: GuestAddress,
    size: usize,
) -> Option<*mut u8> {
    mem.to_region_addr(addr)
        .and_then(|(r, addr)| get_region_host_address_range(r, addr, size))
}

#[cfg(test)]
mod tests {

    use super::*;
    use vm_memory::{GuestAddress, GuestMemoryMmap};

    #[test]
    fn test_get_host_address_range() {
        let start_addr1 = GuestAddress(0x0);
        let start_addr2 = GuestAddress(0x1000);
        let guest_mem =
            GuestMemoryMmap::from_ranges(&[(start_addr1, 0x400), (start_addr2, 0x400)]).unwrap();

        assert!(get_host_address_range(&guest_mem, GuestAddress(0x600), 0x100).is_none());

        // Overlapping range
        assert!(get_host_address_range(&guest_mem, GuestAddress(0x1000), 0x500).is_none());

        // Overlapping range
        assert!(get_host_address_range(&guest_mem, GuestAddress(0x1200), 0x500).is_none());

        let ptr = get_host_address_range(&guest_mem, GuestAddress(0x1000), 0x100).unwrap();

        let ptr0 = get_host_address_range(&guest_mem, GuestAddress(0x1100), 0x100).unwrap();

        let ptr1 = guest_mem.get_host_address(GuestAddress(0x1200)).unwrap();
        assert_eq!(
            ptr,
            guest_mem
                .find_region(GuestAddress(0x1100))
                .unwrap()
                .as_ptr()
        );
        assert_eq!(unsafe { ptr0.offset(0x100) }, ptr1);
    }
}
