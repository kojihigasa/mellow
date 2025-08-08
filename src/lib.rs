pub const HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Mellow Redis Dashboard</title>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <style>
    body { font-family: 'Segoe UI', sans-serif; background: #fff; color: #222; margin: 0; }
    header { background: #f5f5f5; padding: 1rem 2rem; font-size: 1.5rem; border-bottom: 1px solid #eee; position: relative; }
    .metrics-link {
      position: absolute;
      right: 2rem;
      top: 1.2rem;
      font-size: 1rem;
    }
    main { max-width: 1200px; margin: 2rem auto; background: #fff; border-radius: 12px; box-shadow: 0 2px 12px #0001; padding: 2rem; }
    h2 { color: #0074d9; }
    .charts-row {
      display: flex;
      gap: 2rem;
      margin-bottom: 2rem;
      flex-wrap: wrap;
    }
    .chart-container {
      background: #fafbfc;
      border-radius: 8px;
      padding: 1.5rem;
      box-shadow: 0 1px 4px #0001;
      flex: 1 1 0;
      min-width: 320px;
      max-width: 400px;
      height: 220px;
      display: flex;
      flex-direction: column;
      align-items: stretch;
      justify-content: center;
    }
    .chart-container canvas {
      width: 100% !important;
      height: 160px !important;
    }
    #history-table { width: 100%; border-collapse: collapse; margin-top: 2rem; }
    #history-table th, #history-table td { border: 1px solid #e0e0e0; padding: 0.4rem 0.7rem; text-align: right; }
    #history-table th { background: #f5f5f5; color: #0074d9; }
    #history-table td.time { color: #888; font-family: 'Fira Mono', monospace; text-align: left; }
    a { color: #0074d9; text-decoration: none; }
    a:hover { text-decoration: underline; }
  </style>
  <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
</head>
<body>
  <header>
    Mellow Redis Dashboard
    <a class="metrics-link" href="/metrics" target="_blank">Prometheus Metrics</a>
  </header>
  <main>
    <h2>Redis Metrics</h2>
    <div class="charts-row">
      <div class="chart-container">
        <canvas id="commandsChart"></canvas>
      </div>
      <div class="chart-container">
        <canvas id="cpuChart"></canvas>
      </div>
      <div class="chart-container">
        <canvas id="memoryChart"></canvas>
      </div>
    </div>
    <div style="overflow-x:auto;">
      <table id="history-table">
        <thead>
          <tr>
            <th>time</th>
            <th>cpu_usr</th>
            <th>cpu_sys</th>
            <th>clts_con</th>
            <th>clts_blk</th>
            <th>mem</th>
            <th>mem_rss</th>
            <th>rej/s</th>
            <th>cmd/s</th>
            <th>exp/s</th>
            <th>evt/s</th>
            <th>hit/s</th>
            <th>mis/s</th>
          </tr>
        </thead>
        <tbody id="history-tbody">
          <tr><td colspan="13">Loading...</td></tr>
        </tbody>
      </table>
    </div>
  </main>
  <script>
    const evtSource = new EventSource('/events');
    const historyTbody = document.getElementById('history-tbody');

    // Data for history table (max 5 rows)
    const history = [];
    let prev = null;
    let prevTime = null;

    // Data for charts (1 hour = 1800 points if 2s interval)
    const labels = [];
    const commandsData = [];
    const cpuSysData = [];
    const cpuUserData = [];
    const memoryUsedData = [];
    const memoryRssData = [];

    // Unit conversion functions
    function formatBytes(bytes) {
      if (bytes === '' || bytes == null || isNaN(bytes)) return '';
      bytes = Number(bytes);
      if (bytes >= 1 << 30) return (bytes / (1 << 30)).toFixed(2) + ' GB';
      if (bytes >= 1 << 20) return (bytes / (1 << 20)).toFixed(2) + ' MB';
      if (bytes >= 1 << 10) return (bytes / (1 << 10)).toFixed(2) + ' KB';
      return bytes + ' B';
    }
    function formatNumber(n) {
      if (n === '' || n == null || isNaN(n)) return '';
      n = Number(n);
      if (n >= 1e9) return (n / 1e9).toFixed(2) + ' G';
      if (n >= 1e6) return (n / 1e6).toFixed(2) + ' M';
      if (n >= 1e3) return (n / 1e3).toFixed(2) + ' K';
      return n;
    }

    function calcPerSec(curr, prev, currTime, prevTime) {
      if (prev == null || curr == null || prevTime == null) return '';
      const dt = (currTime - prevTime) / 1000;
      if (dt <= 0) return '';
      return ((curr - prev) / dt).toFixed(2);
    }

    // Chart.js options for smooth line, no points, and filled area
    const commandsChart = new Chart(document.getElementById('commandsChart').getContext('2d'), {
      type: 'line',
      data: {
        labels: labels,
        datasets: [
          {
            label: 'cmd/s',
            data: commandsData,
            borderColor: 'rgba(255,133,27,0.9)',
            backgroundColor: 'rgba(255,133,27,0.18)',
            fill: true,
            tension: 0.4,
            borderWidth: 2,
            pointRadius: 0,
            pointHoverRadius: 0,
          }
        ]
      },
      options: {
        plugins: {
          legend: {
            labels: {
              color: '#222',
              usePointStyle: false
            }
          }
        },
        elements: {
          line: { borderWidth: 2, tension: 0.4 },
          point: { radius: 0 }
        },
        scales: {
          x: { ticks: { color: '#222' } },
          y: { 
            ticks: { 
              color: '#222',
              callback: formatNumber
            }
          }
        }
      }
    });

    const cpuChart = new Chart(document.getElementById('cpuChart').getContext('2d'), {
      type: 'line',
      data: {
        labels: labels,
        datasets: [
          {
            label: 'cpu_sys',
            data: cpuSysData,
            borderColor: 'rgba(255,65,54,0.9)',
            backgroundColor: 'rgba(255,65,54,0.18)',
            fill: true,
            tension: 0.4,
            borderWidth: 2,
            pointRadius: 0,
            pointHoverRadius: 0,
          },
          {
            label: 'cpu_usr',
            data: cpuUserData,
            borderColor: 'rgba(46,204,64,0.9)',
            backgroundColor: 'rgba(46,204,64,0.18)',
            fill: true,
            tension: 0.4,
            borderWidth: 2,
            pointRadius: 0,
            pointHoverRadius: 0,
          }
        ]
      },
      options: {
        plugins: {
          legend: {
            labels: {
              color: '#222',
              usePointStyle: false
            }
          }
        },
        elements: {
          line: { borderWidth: 2, tension: 0.4 },
          point: { radius: 0 }
        },
        scales: {
          x: { ticks: { color: '#222' } },
          y: { ticks: { color: '#222' } }
        }
      }
    });

    const memoryChart = new Chart(document.getElementById('memoryChart').getContext('2d'), {
      type: 'line',
      data: {
        labels: labels,
        datasets: [
          {
            label: 'mem',
            data: memoryUsedData,
            borderColor: 'rgba(0,116,217,0.9)',
            backgroundColor: 'rgba(0,116,217,0.18)',
            fill: true,
            tension: 0.4,
            borderWidth: 2,
            pointRadius: 0,
            pointHoverRadius: 0,
          },
          {
            label: 'mem_rss',
            data: memoryRssData,
            borderColor: 'rgba(177,13,201,0.9)',
            backgroundColor: 'rgba(177,13,201,0.18)',
            fill: true,
            tension: 0.4,
            borderWidth: 2,
            pointRadius: 0,
            pointHoverRadius: 0,
          }
        ]
      },
      options: {
        plugins: {
          legend: {
            labels: {
              color: '#222',
              usePointStyle: false
            }
          }
        },
        elements: {
          line: { borderWidth: 2, tension: 0.4 },
          point: { radius: 0 }
        },
        scales: {
          x: { ticks: { color: '#222' } },
          y: { 
            ticks: { 
              color: '#222',
              callback: function(value) { return formatBytes(value); }
            }
          }
        }
      }
    });

    evtSource.onmessage = function(event) {
      try {
        const data = JSON.parse(event.data);
        const now = new Date();
        const nowMs = now.getTime();
        const timeStr = now.toLocaleTimeString();

        // Calculate per second values using diffs
        let cmd_s = '', exp_s = '', evt_s = '', hit_s = '', mis_s = '', rej_s = '';
        if (prev) {
          cmd_s = calcPerSec(data.total_commands_processed, prev.total_commands_processed, nowMs, prevTime);
          exp_s = calcPerSec(data.expired_keys, prev.expired_keys, nowMs, prevTime);
          evt_s = calcPerSec(data.evicted_keys, prev.evicted_keys, nowMs, prevTime);
          hit_s = calcPerSec(data.keyspace_hits, prev.keyspace_hits, nowMs, prevTime);
          mis_s = calcPerSec(data.keyspace_misses, prev.keyspace_misses, nowMs, prevTime);
          rej_s = calcPerSec(data.rejected_connections, prev.rejected_connections, nowMs, prevTime);
        }

        // Prepare row for table
        const row = {
          time: timeStr,
          cpu_usr: data.used_cpu_user !== undefined ? data.used_cpu_user : '',
          cpu_sys: data.used_cpu_sys !== undefined ? data.used_cpu_sys : '',
          clts_con: data.connected_clients ?? '',
          clts_blk: data.blocked_clients ?? '',
          mem: data.used_memory ?? '',
          mem_rss: data.used_memory_rss ?? '',
          'rej/s': rej_s,
          'cmd/s': cmd_s,
          'exp/s': exp_s,
          'evt/s': evt_s,
          'hit/s': hit_s,
          'mis/s': mis_s
        };

        history.unshift(row);
        if (history.length > 5) history.pop();

        historyTbody.innerHTML = history.map(r =>
          `<tr>
            <td class="time">${r.time}</td>
            <td>${r.cpu_usr}</td>
            <td>${r.cpu_sys}</td>
            <td>${formatNumber(r.clts_con)}</td>
            <td>${formatNumber(r.clts_blk)}</td>
            <td>${formatBytes(r.mem)}</td>
            <td>${formatBytes(r.mem_rss)}</td>
            <td>${formatNumber(r['rej/s'])}</td>
            <td>${formatNumber(r['cmd/s'])}</td>
            <td>${formatNumber(r['exp/s'])}</td>
            <td>${formatNumber(r['evt/s'])}</td>
            <td>${formatNumber(r['hit/s'])}</td>
            <td>${formatNumber(r['mis/s'])}</td>
          </tr>`
        ).join('');

        // Update charts (use per second value for cmd/s)
        labels.push(timeStr);
        commandsData.push(Number(cmd_s) || null);
        cpuSysData.push(Number(row.cpu_sys) || null);
        cpuUserData.push(Number(row.cpu_usr) || null);
        memoryUsedData.push(Number(row.mem) || null);
        memoryRssData.push(Number(row.mem_rss) || null);

        if (labels.length > 1800) {
          labels.shift();
          commandsData.shift();
          cpuSysData.shift();
          cpuUserData.shift();
          memoryUsedData.shift();
          memoryRssData.shift();
        }
        commandsChart.update();
        cpuChart.update();
        memoryChart.update();

        prev = data;
        prevTime = nowMs;

      } catch (e) {
        historyTbody.innerHTML = `<tr><td colspan="13">Data fetch error</td></tr>`;
      }
    };
    evtSource.onerror = function() {
      historyTbody.innerHTML = `<tr><td colspan="13">Connection to server lost.</td></tr>`;
    };
  </script>
</body>
</html>
"#;
