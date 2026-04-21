<div align="center">

<img src="assets/icon.png" alt="HUpdater Logo" width="200">

<h1>HUpdater</h1>

<p>A simple and efficient tool for Windows for updating/installing your <a href="https://helium.computer">Helium</a> browser.</p>

</div>

## Features

- **Portable installation**: No need for complex setup, just run the executable.  
    It will copy itself to the `%LocalAppData%\hupdater\` directory and manage its configuration in `%AppData%\hupdater\`.
- **Automatic updates**: HUpdater supports launch interception to ensure that your Helium browser is always up to date without any manual intervention.
- **User-friendly interface**: HUpdater provides a clean and intuitive interface with a one-time setup and real-time status tracking.
- **Lightweight & Fast**: Optimized Rust implementation results in a small binary (~6.5MB) that runs efficiently with minimal system overhead.
- **Multiple update sources**: You can choose to install updates from either GitHub Releases or WinGet.  
    (**Note**: WinGet may not always have the latest version available, so it's recommended to use GitHub Releases for the most up-to-date version.)

## Auto-Update Mechanism

- HUpdater checks for updates by comparing the currently installed version of Helium (read directly from the Windows Registry) with the latest version available on the selected update source (GitHub Releases or WinGet).  
- If a newer version is found, HUpdater will automatically download and install the update for you.
- **!!!You will not lose any data or settings during the update process.!!!**

**How launch interception works**:  
When launch interception is enabled, HUpdater will launch the Helium browser on your behalf.  
(This is done by changing the original shortcut to point to HUpdater instead of Helium. The original Helium icon is explicitly preserved.)

Also, HUpdater uses the same **AppUserModelID** as Helium, so your taskbar will treat them as the same application without any duplicate icons.

## Installation / Usage

1. Download the latest release of HUpdater from the [GitHub Releases](https://github.com/xxanqw/hupdater/releases) page.
2. Just run the downloaded executable. It will automatically copy itself to `%LocalAppData%`.
3. Optionally, you can enable launch interception to keep your Helium browser up to date on every launch.  
     (**Note**: This may cause a slight delay during startup as it checks for updates, but it is highly recommended!)

## Uninstallation

HUpdater includes a built-in uninstaller that:
- Restores original Helium shortcuts (removing the interceptor).
- Closes any active HUpdater processes.
- Completely removes its files from both `%LocalAppData%` and `%AppData%`.
- Shows a confirmation notification once removal is successful.

## Contributing

Contributions are welcome!  
If you have any ideas, suggestions, or bug reports, please feel free to open an issue or submit a pull request!

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

***i code in rust btw.***