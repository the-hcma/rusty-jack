//! `rusty-jack driver` — explicit native-driver test workflows.

use crate::native_driver::{
    print_driver_swap_in_result, print_driver_swap_out_result, swap_in_for_testing,
    swap_out_for_testing,
};
use crate::setup::terminal_is_interactive;
use anyhow::Result;

/// Back up eqMac's HAL driver and install/refresh Rusty Jack's user driver.
pub fn swap_in(json: bool) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    let result = swap_in_for_testing(interactive).map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "driver_swap": result,
        }))?;
        println!("{value}");
    } else {
        print_driver_swap_in_result(&result);
    }

    Ok(())
}

/// Remove Rusty Jack's user driver and restore the backed-up eqMac HAL driver.
pub fn swap_out(json: bool) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    let result = swap_out_for_testing(interactive).map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "driver_swap": result,
        }))?;
        println!("{value}");
    } else {
        print_driver_swap_out_result(&result);
    }

    Ok(())
}
