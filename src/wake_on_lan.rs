//! Wake-on-LAN magic-packet helper for ScalarWebAPI devices.

use crate::RustyJackError;
use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

const WOL_PORT: u16 = 9;

/// Normalize a MAC string to lowercase colon form (`aa:bb:cc:dd:ee:ff`).
///
/// # Errors
///
/// Returns an error when the address is not six bytes.
pub fn normalize_mac_address(value: &str) -> Result<String, RustyJackError> {
    let mac = parse_mac_address(value)?;
    Ok(mac
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":"))
}

/// Build a standard Wake-on-LAN magic packet for `mac_address` (`aa:bb:cc:dd:ee:ff`).
///
/// # Errors
///
/// Returns an error when the MAC address is not six bytes.
pub fn build_magic_packet(mac_address: &str) -> Result<Vec<u8>, RustyJackError> {
    let mac = parse_mac_address(mac_address)?;
    let mut packet = vec![0xFF; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac);
    }
    Ok(packet)
}

fn parse_mac_address(value: &str) -> Result<[u8; 6], RustyJackError> {
    let parts: Vec<&str> = value
        .split([':', '-'])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() != 6 {
        return Err(RustyJackError::Config(format!(
            "invalid MAC address for Wake-on-LAN: {value}"
        )));
    }
    let mut mac = [0_u8; 6];
    for (index, part) in parts.iter().enumerate() {
        mac[index] = u8::from_str_radix(part, 16).map_err(|err| {
            RustyJackError::Config(format!("invalid MAC address byte {part}: {err}"))
        })?;
    }
    Ok(mac)
}

/// Send a Wake-on-LAN magic packet for `mac_address` to the IPv4 broadcast address.
///
/// # Errors
///
/// Returns an error when the packet cannot be built or sent.
pub fn send_wake_on_lan(mac_address: &str) -> Result<(), RustyJackError> {
    let packet = build_magic_packet(mac_address)?;
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(RustyJackError::Io)?;
    socket.set_broadcast(true).map_err(RustyJackError::Io)?;
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(RustyJackError::Io)?;
    let broadcast = (Ipv4Addr::BROADCAST, WOL_PORT);
    let sent = socket
        .send_to(&packet, broadcast)
        .map_err(RustyJackError::Io)?;
    if sent != packet.len() {
        return Err(RustyJackError::AppLaunch(format!(
            "Wake-on-LAN packet truncated: sent {sent} of {} bytes",
            packet.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_magic_packet_colon_mac() {
        let packet = build_magic_packet("aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(packet.len(), 102);
        assert_eq!(&packet[..6], &[0xFF; 6]);
        assert_eq!(&packet[6..12], &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_build_magic_packet_rejects_short_mac() {
        assert!(build_magic_packet("aa:bb:cc").is_err());
    }

    #[test]
    fn test_normalize_mac_address_accepts_dashes() {
        assert_eq!(
            normalize_mac_address("10-4f-a8-f3-01-17").unwrap(),
            "10:4f:a8:f3:01:17"
        );
    }
}
