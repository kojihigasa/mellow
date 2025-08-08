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
    #instance-table { width: 320px; border-collapse: collapse; margin-top: 2rem; }
    #instance-table th, #instance-table td { border: 1px solid #e0e0e0; padding: 0.4rem 0.7rem; text-align: left; }
    #instance-table th { background: #f5f5f5; color: #0074d9; width: 160px; }
    #instance-table td { background: #fff; }
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
            <th>clients</th>
            <th>mem</th>
            <th>mem_rss</th>
            <th>cmd/s</th>
            <th>rej/s</th>
            <th>exp/s</th>
            <th>evt/s</th>
            <th>hit/s</th>
            <th>mis/s</th>
          </tr>
        </thead>
        <tbody id="history-tbody">
          <tr><td colspan="12">Loading...</td></tr>
        </tbody>
      </table>
    </div>
    <div style="overflow-x:auto; margin-top:2rem;">
      <h3>Instance information</h3>
      <table id="instance-table">
        <tbody id="instance-tbody">
          <tr><th>redis_version</th><td>Loading...</td></tr>
          <tr><th>process_id</th><td>Loading...</td></tr>
          <tr><th>uptime_in_seconds</th><td>Loading...</td></tr>
          <tr><th>uptime_in_days</th><td>Loading...</td></tr>
          <tr><th>role</th><td>Loading...</td></tr>
          <tr><th>connected_slaves</th><td>Loading...</td></tr>
        </tbody>
      </table>
    </div>
  </main>
  <script>
    // EventSource for real-time updates
    const evtSource = new EventSource('/events');
    const historyTbody = document.getElementById('history-tbody');

    // History table data (max 5 rows)
    const history = [];
    let prev = null;
    let prevTime = null;

    // Chart data (1 hour = 1800 points if 2s interval)
    const labels = [];
    const commandsData = [];
    const cpuSysData = [];
    const cpuUserData = [];
    const memoryUsedData = [];
    const memoryRssData = [];

    // Instance info state
    let instanceInfo = {
      redis_version: '',
      process_id: '',
      uptime_in_seconds: '',
      uptime_in_days: '',
      role: '',
      connected_slaves: ''
    };

    // Format bytes to human-readable string
    function formatBytes(bytes) {
      if (bytes === '' || bytes == null || isNaN(bytes)) return '';
      bytes = Number(bytes);
      if (bytes >= 1 << 30) return (bytes / (1 << 30)).toFixed(2) + ' GB';
      if (bytes >= 1 << 20) return (bytes / (1 << 20)).toFixed(2) + ' MB';
      if (bytes >= 1 << 10) return (bytes / (1 << 10)).toFixed(2) + ' KB';
      return bytes + ' B';
    }
    // Format numbers with K/M/G suffix
    function formatNumber(n) {
      if (n === '' || n == null || isNaN(n)) return '';
      n = Number(n);
      if (n >= 1e9) return (n / 1e9).toFixed(2) + ' G';
      if (n >= 1e6) return (n / 1e6).toFixed(2) + ' M';
      if (n >= 1e3) return (n / 1e3).toFixed(2) + ' K';
      return n;
    }
    // Truncate to 2 decimal places, always show 2 digits
    function truncate2(n) {
      if (n === '' || n == null || isNaN(n)) return '';
      return (Math.floor(Number(n) * 100) / 100).toFixed(2);
    }
    // Difference between current and previous value
    function diff(curr, prev) {
      if (prev == null || curr == null) return '';
      return Number(curr) - Number(prev);
    }
    // Convert to integer (for chart and table)
    function toInt(n) {
      if (n === '' || n == null || isNaN(n)) return null;
      return Math.trunc(Number(n));
    }

    // Chart.js for cmd/s (y-axis: only multiples of 5, always show the top label)
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
            beginAtZero: true,
            min: 0,
            afterDataLimits: function(axis) {
              let max = Math.max(...commandsData.filter(v => typeof v === 'number' && !isNaN(v)), 0);
              let top = Math.ceil(max / 5) * 5;
              axis.max = top < 5 ? 5 : top;
              axis.min = 0;
            },
            ticks: {
              color: '#222',
              stepSize: 5,
              callback: function(value, index, ticks) {
                // Always show the top tick label
                const maxTick = ticks[ticks.length - 1]?.value;
                if (value === maxTick) {
                  return value;
                }
                // Show only multiples of 5, unique, integer
                if (value % 5 !== 0) return '';
                value = Math.trunc(value);
                if (index === 0 || value !== Math.trunc(ticks[index - 1].value)) {
                  return value;
                }
                return '';
              }
            }
          }
        }
      }
    });

    // Chart.js for cpu (y-axis: only multiples of 5, unique, integer, start at 0)
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
          y: { 
            beginAtZero: true,
            ticks: {
              color: '#222',
              stepSize: 5,
              callback: function(value, index, ticks) {
                // Show only multiples of 5, unique, integer
                if (value % 5 !== 0) return '';
                value = Math.trunc(value);
                if (index === 0 || value !== Math.trunc(ticks[index - 1].value)) {
                  return value;
                }
                return '';
              }
            }
          }
        }
      }
    });

    // Chart.js for memory (y-axis: default Chart.js ticks, just format as bytes)
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
            beginAtZero: true,
            min: 0,
            ticks: {
              color: '#222',
              callback: function(value) {
                return formatBytes(value);
              }
            }
          }
        }
      }
    });

    // Handle SSE events and update table/charts
    evtSource.onmessage = function(event) {
      try {
        const data = JSON.parse(event.data);
        const now = new Date();
        const nowMs = now.getTime();
        const timeStr = now.toLocaleTimeString();

        // Calculate difference values using previous data
        let cmd_s = '', exp_s = '', evt_s = '', hit_s = '', mis_s = '', rej_s = '';
        if (prev) {
          cmd_s = diff(data.total_commands_processed, prev.total_commands_processed);
          exp_s = diff(data.expired_keys, prev.expired_keys);
          evt_s = diff(data.evicted_keys, prev.evicted_keys);
          hit_s = diff(data.keyspace_hits, prev.keyspace_hits);
          mis_s = diff(data.keyspace_misses, prev.keyspace_misses);
          rej_s = diff(data.rejected_connections, prev.rejected_connections);
        }

        // Prepare row for table
        const row = {
          time: timeStr,
          cpu_usr: data.used_cpu_user !== undefined ? truncate2(data.used_cpu_user) : '',
          cpu_sys: data.used_cpu_sys !== undefined ? truncate2(data.used_cpu_sys) : '',
          clients: data.connected_clients ?? '',
          mem: data.used_memory ?? '',
          mem_rss: data.used_memory_rss ?? '',
          'cmd/s': toInt(cmd_s),
          'rej/s': toInt(rej_s),
          'exp/s': toInt(exp_s),
          'evt/s': toInt(evt_s),
          'hit/s': toInt(hit_s),
          'mis/s': toInt(mis_s)
        };

        history.unshift(row);
        if (history.length > 5) history.pop();

        historyTbody.innerHTML = history.map(r =>
          `<tr>
            <td class="time">${r.time}</td>
            <td>${r.cpu_usr}</td>
            <td>${r.cpu_sys}</td>
            <td>${formatNumber(r.clients)}</td>
            <td>${formatBytes(r.mem)}</td>
            <td>${formatBytes(r.mem_rss)}</td>
            <td>${formatNumber(r['cmd/s'])}</td>
            <td>${formatNumber(r['rej/s'])}</td>
            <td>${formatNumber(r['exp/s'])}</td>
            <td>${formatNumber(r['evt/s'])}</td>
            <td>${formatNumber(r['hit/s'])}</td>
            <td>${formatNumber(r['mis/s'])}</td>
          </tr>`
        ).join('');

        // Update instance info
        instanceInfo.redis_version = data.redis_version ?? '';
        instanceInfo.process_id = data.process_id ?? '';
        instanceInfo.uptime_in_seconds = data.uptime_in_seconds ?? '';
        instanceInfo.uptime_in_days = data.uptime_in_days ?? '';
        instanceInfo.role = data.role ?? '';
        instanceInfo.connected_slaves = data.connected_slaves ?? '';

        document.getElementById('instance-tbody').innerHTML = `
          <tr><th>redis_version</th><td>${instanceInfo.redis_version}</td></tr>
          <tr><th>process_id</th><td>${instanceInfo.process_id}</td></tr>
          <tr><th>uptime_in_seconds</th><td>${instanceInfo.uptime_in_seconds}</td></tr>
          <tr><th>uptime_in_days</th><td>${instanceInfo.uptime_in_days}</td></tr>
          <tr><th>role</th><td>${instanceInfo.role}</td></tr>
          <tr><th>connected_slaves</th><td>${instanceInfo.connected_slaves}</td></tr>
        `;

        // Update charts (use integer for cmd/s)
        labels.push(timeStr);
        commandsData.push(toInt(cmd_s));
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
        historyTbody.innerHTML = `<tr><td colspan="12">Data fetch error</td></tr>`;
      }
    };
    evtSource.onerror = function() {
      historyTbody.innerHTML = `<tr><td colspan="12">Connection to server lost.</td></tr>`;
    };
  </script>
</body>
</html>
"#;
