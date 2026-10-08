use crate::command::Command;
use crate::inverter::Inverter;
use crate::metric_collector::MetricCollector;
use log::{error, info};
use std::time::Duration;

/// Pause after a stale reading, doubled for each further stale one up to STALE_BACKOFF_MAX.
/// Polling again within ~30 s restarts the DTU's lockout; about a minute without requests
/// let it recover in measurements.
pub const STALE_BACKOFF: Duration = Duration::from_secs(60);
pub const STALE_BACKOFF_MAX: Duration = Duration::from_secs(600);

/// Warnings are fetched with every n-th successful reading (about every 5 minutes by default)
pub const WARNINGS_EVERY_NTH_READING: u64 = 10;

/// Time to wait before the next reading
pub fn next_delay(interval: Duration, consecutive_stale: u32) -> Duration {
    if consecutive_stale == 0 {
        return interval;
    }
    let backoff = STALE_BACKOFF.saturating_mul(1 << (consecutive_stale - 1).min(10));
    backoff.min(STALE_BACKOFF_MAX).max(interval)
}

/// Polls the inverter and passes readings, warnings and commands between it and the outputs.
pub struct Poller<'a> {
    inverter: Inverter<'a>,
    outputs: Vec<Box<dyn MetricCollector>>,
    interval: Duration,
    readings: u64,
    consecutive_stale: u32,
}

impl<'a> Poller<'a> {
    pub fn new(
        inverter: Inverter<'a>,
        outputs: Vec<Box<dyn MetricCollector>>,
        interval: Duration,
    ) -> Self {
        Self {
            inverter,
            outputs,
            interval,
            readings: 0,
            consecutive_stale: 0,
        }
    }

    /// Runs one cycle and returns the time to wait before the next one.
    pub fn cycle(&mut self) -> Duration {
        // a command takes the place of this cycle's reading, so that the DTU doesn't get more
        // requests than its rate limit allows
        let commands: Vec<Command> = self
            .outputs
            .iter_mut()
            .flat_map(|output| output.commands())
            .collect();
        if let Some(Command::SetPowerLimit(percent)) = commands.last() {
            if let Err(e) = self.inverter.set_power_limit(*percent) {
                error!("could not set the power limit: {e}");
            }
            return self.interval;
        }

        let reading = self.inverter.update_state();
        if self.inverter.last_reading_stale() {
            self.consecutive_stale += 1;
        } else {
            self.consecutive_stale = 0;
        }
        let mut delay = next_delay(self.interval, self.consecutive_stale);
        if self.consecutive_stale > 0 {
            info!(
                "DTU served stale data, waiting {}s before the next request",
                delay.as_secs()
            );
        }
        if let Some(r) = reading {
            self.outputs
                .iter_mut()
                .for_each(|output| output.publish(&r));

            // warnings rarely change; every request counts towards the DTU's rate limit
            if self.readings.is_multiple_of(WARNINGS_EVERY_NTH_READING) {
                if let Some(warnings) = self.inverter.fetch_warnings() {
                    self.outputs
                        .iter_mut()
                        .for_each(|output| output.publish_warnings(&r, &warnings));
                }
                // keep the next reading clear of the DTU's ~30 s window after this request
                delay += Duration::from_secs(1);
            }
            self.readings += 1;
        }
        delay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inverter::FRAME_HEADER_LENGTH;
    use crate::protos::hoymiles::{
        CommandPB::{CommandReqDTO, CommandResDTO},
        RealData::{HMSStateResponse, InverterState, Warning, WarningsResponse},
    };
    use crate::test_support::{fake_dtu, frame_reply, reply_to, request_command, Reply};
    use protobuf::Message;
    use std::cell::RefCell;
    use std::rc::Rc;

    const INTERVAL: Duration = Duration::from_millis(30_500);

    /// What the outputs received
    #[derive(Default)]
    struct Log {
        published: Vec<i32>,
        warnings: Vec<usize>,
    }

    struct TestOutput {
        log: Rc<RefCell<Log>>,
        commands: Vec<Command>,
    }

    impl MetricCollector for TestOutput {
        fn publish(&mut self, hms_state: &HMSStateResponse) {
            self.log
                .borrow_mut()
                .published
                .push(hms_state.pv_current_power);
        }

        fn publish_warnings(&mut self, _hms_state: &HMSStateResponse, warnings: &[Warning]) {
            self.log.borrow_mut().warnings.push(warnings.len());
        }

        fn commands(&mut self) -> Vec<Command> {
            std::mem::take(&mut self.commands)
        }
    }

    fn reading(power: i32, link: i32) -> HMSStateResponse {
        let mut response = HMSStateResponse::new();
        response.pv_current_power = power;
        let mut inverter = InverterState::new();
        inverter.link = link;
        response.inverter_state.push(inverter);
        response
    }

    fn fresh(power: i32) -> Reply {
        Box::new(move |req| reply_to(req, &reading(power, 1)))
    }

    fn stale() -> Reply {
        Box::new(|req| reply_to(req, &reading(0, 0)))
    }

    fn warnings(count: usize) -> Reply {
        Box::new(move |req| {
            let mut response = WarningsResponse::new();
            response.warnings = vec![Warning::new(); count];
            frame_reply(req, 0xa204, &response.write_to_bytes().unwrap())
        })
    }

    fn command_accepted() -> Reply {
        Box::new(|req| frame_reply(req, 0xa205, &CommandReqDTO::new().write_to_bytes().unwrap()))
    }

    /// A poller for a fake DTU with `replies`, one output with `commands` queued
    fn poller(port: u16, commands: Vec<Command>) -> (Poller<'static>, Rc<RefCell<Log>>) {
        let log = Rc::new(RefCell::new(Log::default()));
        let output = TestOutput {
            log: log.clone(),
            commands,
        };
        let inverter = Inverter::with_port("127.0.0.1", port);
        (Poller::new(inverter, vec![Box::new(output)], INTERVAL), log)
    }

    #[test]
    fn first_reading_is_published_with_warnings() {
        let (port, dtu) = fake_dtu(vec![fresh(5500), warnings(2), fresh(5600)]);
        let (mut poller, log) = poller(port, vec![]);
        // warnings are fetched right after the reading; the next pause is a second longer
        assert_eq!(poller.cycle(), INTERVAL + Duration::from_secs(1));
        assert_eq!(poller.cycle(), INTERVAL);
        assert_eq!(log.borrow().published, [5500, 5600]);
        assert_eq!(log.borrow().warnings, [2]);
        let commands: Vec<u16> = dtu
            .join()
            .unwrap()
            .iter()
            .map(|r| request_command(r))
            .collect();
        assert_eq!(commands, [0xa311, 0xa304, 0xa311]);
    }

    #[test]
    fn warnings_are_fetched_every_nth_reading() {
        let mut replies = vec![fresh(1), warnings(0)];
        for n in 2..=WARNINGS_EVERY_NTH_READING {
            replies.push(fresh(n as i32));
        }
        replies.push(fresh(11));
        replies.push(warnings(1));
        let (port, dtu) = fake_dtu(replies);
        let (mut poller, log) = poller(port, vec![]);
        for _ in 0..=WARNINGS_EVERY_NTH_READING {
            poller.cycle();
        }
        assert_eq!(log.borrow().published.len(), 11);
        assert_eq!(log.borrow().warnings, [0, 1]);
        let warning_requests = dtu
            .join()
            .unwrap()
            .iter()
            .filter(|r| request_command(r) == 0xa304)
            .count();
        assert_eq!(warning_requests, 2);
    }

    #[test]
    fn failing_warnings_request_still_publishes_the_reading() {
        let (port, dtu) = fake_dtu(vec![fresh(100), Box::new(|_| Vec::new())]);
        let (mut poller, log) = poller(port, vec![]);
        assert_eq!(poller.cycle(), INTERVAL + Duration::from_secs(1));
        assert_eq!(log.borrow().published, [100]);
        assert!(log.borrow().warnings.is_empty());
        dtu.join().unwrap();
    }

    #[test]
    fn stale_readings_back_off_until_a_fresh_one() {
        let (port, dtu) = fake_dtu(vec![
            fresh(1),
            warnings(0),
            stale(),
            stale(),
            stale(),
            fresh(2),
        ]);
        let (mut poller, log) = poller(port, vec![]);
        poller.cycle();
        assert_eq!(poller.cycle(), Duration::from_secs(60));
        assert_eq!(poller.cycle(), Duration::from_secs(120));
        assert_eq!(poller.cycle(), Duration::from_secs(240));
        assert_eq!(poller.cycle(), INTERVAL);
        // stale readings aren't published
        assert_eq!(log.borrow().published, [1, 2]);
        dtu.join().unwrap();
    }

    #[test]
    fn unreachable_inverter_keeps_the_normal_interval() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (mut poller, log) = poller(port, vec![]);
        assert_eq!(poller.cycle(), INTERVAL);
        assert!(log.borrow().published.is_empty());
    }

    #[test]
    fn command_takes_the_place_of_the_reading() {
        let (port, dtu) = fake_dtu(vec![command_accepted(), fresh(7), warnings(0)]);
        let commands = vec![Command::SetPowerLimit(80), Command::SetPowerLimit(50)];
        let (mut poller, log) = poller(port, commands);
        assert_eq!(poller.cycle(), INTERVAL);
        assert!(log.borrow().published.is_empty());
        poller.cycle();
        assert_eq!(log.borrow().published, [7]);

        let requests = dtu.join().unwrap();
        assert_eq!(request_command(&requests[0]), 0xa305);
        // only the latest command is sent
        let command = CommandResDTO::parse_from_bytes(&requests[0][FRAME_HEADER_LENGTH..]).unwrap();
        assert_eq!(command.data, "A:500,B:0,C:0\r");
        assert_eq!(request_command(&requests[1]), 0xa311);
    }

    #[test]
    fn rejected_command_keeps_polling() {
        let rejected: Reply = Box::new(|req| {
            let mut response = CommandReqDTO::new();
            response.err_code = 3;
            frame_reply(req, 0xa205, &response.write_to_bytes().unwrap())
        });
        let (port, dtu) = fake_dtu(vec![rejected, fresh(9), warnings(0)]);
        let (mut poller, log) = poller(port, vec![Command::SetPowerLimit(50)]);
        assert_eq!(poller.cycle(), INTERVAL);
        poller.cycle();
        assert_eq!(log.borrow().published, [9]);
        dtu.join().unwrap();
    }

    #[test]
    fn back_off_sequence() {
        assert_eq!(next_delay(INTERVAL, 0), INTERVAL);
        assert_eq!(next_delay(INTERVAL, 1), Duration::from_secs(60));
        assert_eq!(next_delay(INTERVAL, 2), Duration::from_secs(120));
        assert_eq!(next_delay(INTERVAL, 4), Duration::from_secs(480));
        assert_eq!(next_delay(INTERVAL, 5), STALE_BACKOFF_MAX);
        assert_eq!(next_delay(INTERVAL, 40), STALE_BACKOFF_MAX);
        // a configured interval longer than the back-off wins
        let long = Duration::from_secs(900);
        assert_eq!(next_delay(long, 1), long);
    }

    #[test]
    fn default_collector_methods_do_nothing() {
        struct PublishOnly;
        impl MetricCollector for PublishOnly {
            fn publish(&mut self, _hms_state: &HMSStateResponse) {}
        }
        let mut output = PublishOnly;
        output.publish(&HMSStateResponse::new());
        output.publish_warnings(&HMSStateResponse::new(), &[Warning::new()]);
        assert!(output.commands().is_empty());
    }
}
