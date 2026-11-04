use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result};
use razerr_ui::Activation;
use tokio::sync::mpsc;
use zbus::{Connection, wire::OwnedVariant};

const APP_NAME: &str = "org.razerr_rs.App";
const APP_PATH: &str = "/org/razerr_rs/App";
const APP_INTERFACE: &str = "org.freedesktop.Application";
type Platform = HashMap<String, OwnedVariant>;

struct Application {
    sender: mpsc::Sender<Activation>,
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl Application {
    async fn activate(&self, platform_data: Platform) -> zbus::fdo::Result<()> {
        drop(platform_data);
        self.send(Activation::Present).await
    }
    async fn activate_action(
        &self,
        action_name: &str,
        parameter: Vec<OwnedVariant>,
        platform_data: Platform,
    ) -> zbus::fdo::Result<()> {
        drop(platform_data);
        if !parameter.is_empty() {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Application actions take no parameters".into(),
            ));
        }
        let action = match action_name {
            "settings" => Activation::Settings,
            "quit" => Activation::Quit,
            _ => {
                return Err(zbus::fdo::Error::InvalidArgs(format!(
                    "Unknown application action: {action_name}"
                )));
            }
        };
        self.send(action).await
    }
}

impl Application {
    async fn send(&self, activation: Activation) -> zbus::fdo::Result<()> {
        self.sender
            .send(activation)
            .await
            .map_err(|_| zbus::fdo::Error::Failed("Window has closed".into()))
    }
}

pub async fn acquire(
    sender: mpsc::Sender<Activation>,
    settings: bool,
) -> Result<Option<Connection>> {
    match zbus::connection::Builder::session()
        .serve_at(APP_PATH, Application { sender })
        .name(APP_NAME)
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .method_timeout(Duration::from_secs(5))
        .build()
        .await
    {
        Ok(connection) => Ok(Some(connection)),
        Err(zbus::Error::NameTaken) => {
            let connection = zbus::connection::Builder::session()
                .method_timeout(Duration::from_secs(5))
                .build()
                .await?;
            let Some(owner) = unique_owner(&connection, APP_NAME).await? else {
                return Ok(None);
            };
            if settings {
                action(&connection, &owner, "settings").await?;
            } else {
                connection
                    .call_method(
                        Some(owner.as_str()),
                        APP_PATH,
                        Some(APP_INTERFACE),
                        "Activate",
                        &(Platform::new(),),
                    )
                    .await?;
            }
            Ok(None)
        }
        Err(error) => {
            Err(error).context("could not register the desktop application on the session bus")
        }
    }
}

async fn action(connection: &Connection, owner: &str, action: &str) -> Result<()> {
    connection
        .call_method(
            Some(owner),
            APP_PATH,
            Some(APP_INTERFACE),
            "ActivateAction",
            &(action, Vec::<OwnedVariant>::new(), Platform::new()),
        )
        .await?;
    Ok(())
}

pub async fn quit() -> Result<()> {
    let connection = zbus::connection::Builder::session()
        .method_timeout(Duration::from_secs(5))
        .build()
        .await?;
    if let Some(owner) = unique_owner(&connection, APP_NAME).await? {
        action(&connection, &owner, "quit").await?;
    }
    Ok(())
}

async fn unique_owner(connection: &Connection, name: &str) -> Result<Option<String>> {
    let bus = zbus::fdo::DBusProxy::new(connection).await?;
    match bus.get_name_owner(name.try_into()?).await {
        Ok(owner) => Ok(Some(owner.to_string())),
        Err(zbus::fdo::Error::NameHasNoOwner(_)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
