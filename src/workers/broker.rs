// RGB Node: sovereign smart contracts backend
//
// SPDX-License-Identifier: Apache-2.0
//
// Designed in 2020-2025 by Dr Maxim Orlovsky <orlovsky@lnp-bp.org>
// Written in 2020-2025 by Dr Maxim Orlovsky <orlovsky@lnp-bp.org>
//
// Copyright (C) 2020-2024 LNP/BP Standards Association. All rights reserved.
// Copyright (C) 2025 RGB Consortium, Switzerland. All rights reserved.
// Copyright (C) 2020-2025 Dr Maxim Orlovsky. All rights reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except
// in compliance with the License. You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the License
// is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express
// or implied. See the License for the specific language governing permissions and limitations under
// the License.

use std::io;
#[cfg(feature = "embedded")]
use std::thread::JoinHandle;
use std::time::Duration;

use amplify::IoError;
use amplify::confinement::MediumVec;
#[cfg(feature = "embedded")]
use crossbeam_channel::Sender;
use crossbeam_channel::{Receiver, select};
use microservices::UThread;
#[cfg(feature = "server")]
use netservices::{NetAccept, service};
use rgb::{Pile, Stockpile};
use rgbrpc::{ContractReply, Failure, RgbRpcReq, RgbRpcResp};

#[cfg(feature = "server")]
use crate::Dispatcher;
#[cfg(feature = "embedded")]
use crate::services::{AsyncClient, AsyncDispatcher};
use crate::services::{ContractsReader, ContractsWriter, Reader2Broker, ReaderMsg, Request2Reader};
use crate::{Config, ReqId};

pub struct Broker<Sp: Stockpile>
where
    Sp: Stockpile + Send + 'static,
    Sp::Stock: Send,
    Sp::Pile: Send,
    <Sp::Pile as Pile>::Seal: Send + serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    #[cfg(feature = "embedded")]
    rpc_thread: UThread<AsyncDispatcher>,
    #[cfg(feature = "embedded")]
    rpc_tx: Sender<(ReqId, RgbRpcResp)>,

    #[cfg(not(feature = "embedded"))]
    rpc_thread: service::Runtime<(ReqId, RgbRpcResp)>,
    rpc_rx: Receiver<(ReqId, RgbRpcReq)>,

    reader_rx: Receiver<Reader2Broker<<Sp::Pile as Pile>::Seal>>,
    reader_thread: UThread<ContractsReader<<Sp::Pile as Pile>::Seal>>,
    writer_thread: UThread<ContractsWriter<Sp>>,
}

impl<Sp> Broker<Sp>
where
    Sp: Stockpile + Send + 'static,
    Sp::Stock: Send,
    Sp::Pile: Send,
    <Sp::Pile as Pile>::Seal: Send + serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    #[cfg(feature = "embedded")]
    pub fn start_embedded(conf: Config, stockpile: Sp) -> Result<Self, BrokerError> {
        Self::start_inner(conf, stockpile)
    }

    #[cfg(not(feature = "embedded"))]
    pub fn run_standalone(conf: Config, stockpile: Sp) -> Result<(), BrokerError> {
        let me = Self::start_inner(conf, stockpile)?;
        me.run_internal()
    }

    #[cfg(feature = "embedded")]
    pub fn run(self) -> io::Result<JoinHandle<Result<(), BrokerError>>> {
        std::thread::Builder::new()
            .name(s!("broker"))
            .spawn(move || self.run_internal())
    }

    #[cfg(feature = "embedded")]
    pub fn client(&self) -> AsyncClient { AsyncClient::new(self.rpc_thread.sender()) }

    fn start_inner(conf: Config, stockpile: Sp) -> Result<Self, BrokerError> {
        const TIMEOUT: Option<Duration> = Some(Duration::from_secs(60 * 10));

        log::info!("Starting contracts reader thread...");
        let (reader_tx, reader_rx) =
            crossbeam_channel::unbounded::<Reader2Broker<<Sp::Pile as Pile>::Seal>>();
        let reader = ContractsReader::new(reader_tx);
        let reader_thread = UThread::new(reader, TIMEOUT);

        log::info!("Starting contracts writer thread...");
        let writer = ContractsWriter::new(stockpile, reader_thread.sender());
        let writer_thread = UThread::new(writer, TIMEOUT);

        log::info!("Starting the dispatcher thread...");
        let (rpc_tx, rpc_rx) = crossbeam_channel::unbounded::<(ReqId, RgbRpcReq)>();
        #[cfg(feature = "embedded")]
        let (rpc_thread, rpc_tx) = {
            let (rpc_tx2, rpc_rx2) = crossbeam_channel::unbounded::<(ReqId, RgbRpcResp)>();
            let dispatcher = AsyncDispatcher::new(rpc_tx, rpc_rx2);
            let thread = UThread::new(dispatcher, TIMEOUT);
            (thread, rpc_tx2)
        };
        #[cfg(not(feature = "embedded"))]
        let rpc_thread = {
            let controller = Dispatcher::new(conf.network, rpc_tx);
            let listen = conf.rpc.iter().map(|addr| {
                NetAccept::bind(addr)
                    .unwrap_or_else(|err| panic!("unable to bind to {addr}: {err}"))
            });
            let rpc_runtime = service::Runtime::new(conf.rpc[0], controller, listen)
                .map_err(|err| BrokerError::Dispatcher(err.into()))?;
            rpc_runtime
        };

        log::info!("Launch completed successfully");
        Ok(Self {
            rpc_thread,
            rpc_rx,
            #[cfg(feature = "embedded")]
            rpc_tx,
            reader_rx,
            reader_thread,
            writer_thread,
        })
    }

    fn run_internal(mut self) -> Result<(), BrokerError> {
        select! {
            recv(self.rpc_rx) -> msg => {
                match msg {
                    Ok((req_id, msg)) => { self.proc_rpc_msg(req_id, msg).expect("unable to send a message"); },
                    Err(err) => {
                        log::error!("Error receiving RPC message: {err}");
                    }
                }
            },
            recv(self.reader_rx) -> msg => {
                match msg {
                    Ok(msg) => { self.proc_reader_msg(msg).expect("unable to send a message"); },
                    Err(err) => {
                        log::error!("Error receiving reader message: {err}");
                    }
                }
            }
        }

        self.rpc_thread
            .join()
            .map_err(|_| BrokerError::Thread("Dispatcher"))?;
        self.reader_thread
            .join()
            .map_err(|_| BrokerError::Thread("Contracts reader"))?;
        self.writer_thread
            .join()
            .map_err(|_| BrokerError::Thread("Contracts writer"))?;
        Ok(())
    }

    fn proc_rpc_msg(&mut self, req_id: ReqId, msg: RgbRpcReq) -> io::Result<()> {
        log::debug!("Received an RPC message: {msg}");
        match msg {
            RgbRpcReq::State(contract_id) => {
                if let Err(err) = self
                    .reader_thread
                    .sender()
                    .try_send(Request2Reader::ReadState(req_id, contract_id))
                {
                    log::error!("Unable to send a request to the reader thread: {err}");
                    self.send_rpc_resp(
                        req_id,
                        RgbRpcResp::Failure(Failure::not_found(contract_id)),
                    );
                }
            }
            _ => todo!(),
        }
        Ok(())
    }

    fn proc_reader_msg(&mut self, resp: Reader2Broker<<Sp::Pile as Pile>::Seal>) -> io::Result<()> {
        log::debug!("Received reply from a reader for an RPC request {}", resp.req_id());
        match (resp.req_id(), resp.into_reply()) {
            (req_id, ReaderMsg::State(contract_id, state)) => {
                // TODO: Move from bincode to strict encoding, which requires implementation of
                //       Strict(En/De)code for TypedVal, and switching ContractState from using
                //       StrictVal to TypedVal
                let serialized = bincode::serde::encode_to_vec(&state, bincode::config::standard())
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                let resp = match MediumVec::try_from(serialized) {
                    Ok(data) => RgbRpcResp::State(ContractReply { contract_id, data }),
                    Err(_) => {
                        log::warn!("Contract state for {contract_id} exceeds the 16MB limit");
                        RgbRpcResp::Failure(Failure::too_large(contract_id))
                    }
                };
                self.send_rpc_resp(req_id, resp);
            }
            (req_id, ReaderMsg::NotFound(id)) => {
                self.send_rpc_resp(req_id, RgbRpcResp::Failure(Failure::not_found(id)));
            }
        }
        Ok(())
    }

    fn send_rpc_resp(&mut self, req_id: ReqId, response: RgbRpcResp) {
        if let Err(err) = {
            #[cfg(not(feature = "embedded"))]
            {
                self.rpc_thread.cmd((req_id, response))
            }
            #[cfg(feature = "embedded")]
            {
                self.rpc_tx.send((req_id, response))
            }
        } {
            log::error!("Channel to the dispatcher thread is broken: {err}");
            panic!("The channel to the dispatcher thread is broken. Unable to proceed, exiting.");
        };
    }
}

#[derive(Debug, Display, Error, From)]
#[display(doc_comments)]
pub enum BrokerError {
    /// unable to initialize dispatcher.
    ///
    /// {0}
    Dispatcher(IoError),

    /// unable to initialize importing service.
    ///
    /// {0}
    Import(IoError),

    /// unable to initialize block importing service.
    ///
    /// {0}
    Importer(IoError),

    /// unable to create a thread for {0}.
    Thread(&'static str),
}
