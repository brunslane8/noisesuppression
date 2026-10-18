fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Hush");
        res.set("FileDescription", "Hush - microphone noise suppressor");
        let _ = res.compile();
    }
}
