use std::error::Error;

#[derive(Clone)]
pub enum ConanNotif {
    Text(String, String),
    Sys(String),
}

impl ConanNotif {
    /// Used to push notifications to D-Bus
    /// # Errors
    pub async fn notify(&self) -> Result<(), Box<dyn Error>> {
        match self {
            ConanNotif::Text(name, msg) => {
                let mut notif = notify_rust::Notification::new();
                notif.body = msg.into();
                notif.summary = name.into();
                let notif = notif.finalize();
                notif.show_async().await?;
            }
            ConanNotif::Sys(msg) => {
                let mut notif = notify_rust::Notification::new();
                notif.body = msg.clone();
                notif.appname = "Conan".to_string();
                let notif = notif.finalize();
                notif.show_async().await?;
            }
        }
        Ok(())
    }
}
